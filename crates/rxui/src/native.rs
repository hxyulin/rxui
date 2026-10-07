use crate::{
    AccessKitTree, AppContext, Bounds, Color, EffectCycle, ElementId, Entity, PointerEvent,
    ReadContext, Runtime, SemanticAction, SpawnError, TaskExecutor, TextInputEvent, TextMeasure,
    TextMovement, Theme, ThreadPoolExecutor, Ui, UiError, UiPainter, View,
};
use astrelis::{Frame, GraphicsContext, wgpu};
use astrelis_winit::{
    AppContext as NativeContext, Handler, PrepareAction, Runner, RunnerOptions, SurfaceSettings,
    WindowInfo, WindowMetrics,
    winit::{
        dpi::{LogicalPosition, LogicalSize, Size},
        event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
        keyboard::{Key, ModifiersState, NamedKey},
        window::{Window, WindowAttributes, WindowId as NativeWindowId},
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    fmt,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

/// Failure in native hosting, initialization or a queued application operation.
#[derive(Debug)]
pub enum ApplicationError {
    /// This runtime is not attached to a native Application.
    NoHost,
    /// The application host has shut down.
    Exited,
    /// Window dimensions/background/settings are invalid.
    InvalidWindowOptions,
    /// Requested window has already closed.
    ClosedWindow,
    /// UI state/layout/painting failed.
    Ui(UiError),
    /// Deferred effects exceeded their callback budget.
    Effects(EffectCycle),
    /// Task execution could not be configured.
    Tasks(SpawnError),
    /// Native lifecycle/window/loop failed.
    Native(Box<dyn Error>),
}
impl fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHost => f.write_str("RXUI runtime has no native application host"),
            Self::Exited => f.write_str("RXUI application has exited"),
            Self::InvalidWindowOptions => f.write_str("invalid RXUI window options"),
            Self::ClosedWindow => f.write_str("RXUI window has closed"),
            Self::Ui(e) => e.fmt(f),
            Self::Effects(e) => e.fmt(f),
            Self::Tasks(e) => e.fmt(f),
            Self::Native(e) => e.fmt(f),
        }
    }
}
impl Error for ApplicationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Ui(e) => Some(e),
            Self::Effects(e) => Some(e),
            Self::Tasks(e) => Some(e),
            Self::Native(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}
impl From<UiError> for ApplicationError {
    fn from(e: UiError) -> Self {
        Self::Ui(e)
    }
}
impl From<astrelis::Error> for ApplicationError {
    fn from(e: astrelis::Error) -> Self {
        Self::Ui(e.into())
    }
}
impl From<crate::AccessError> for ApplicationError {
    fn from(e: crate::AccessError) -> Self {
        Self::Ui(e.into())
    }
}
impl From<EffectCycle> for ApplicationError {
    fn from(e: EffectCycle) -> Self {
        Self::Effects(e)
    }
}
impl From<SpawnError> for ApplicationError {
    fn from(e: SpawnError) -> Self {
        Self::Tasks(e)
    }
}

/// Native window options, independent of a concrete component type.
/// Creation is queued until the current update ends; platform/GPU creation errors
/// are returned by Application::run, while invalid options are rejected immediately.
#[derive(Clone, Debug)]
pub struct WindowOptions {
    attributes: WindowAttributes,
    background: Option<Color>,
    theme: Option<Theme>,
    surface: SurfaceSettings,
}
impl Default for WindowOptions {
    fn default() -> Self {
        Self::new()
    }
}
impl WindowOptions {
    /// Default 640×480 window inheriting the application theme, with a single-sample surface.
    pub fn new() -> Self {
        Self {
            attributes: Window::default_attributes()
                .with_title("RXUI")
                .with_inner_size(LogicalSize::new(640., 480.)),
            background: None,
            theme: None,
            surface: SurfaceSettings::new(),
        }
    }
    /// Native window title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.attributes = self.attributes.with_title(title);
        self
    }
    /// Initial logical inner dimensions, finite and positive.
    #[must_use]
    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.attributes = self
            .attributes
            .with_inner_size(LogicalSize::new(width, height));
        self
    }
    /// Linear RGBA clear color for the host's UI pass.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
    /// Explicit window theme, independent of later application theme changes.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
    /// Surface-owned MSAA/depth configuration, validated by Astrelis at creation.
    #[must_use]
    pub fn surface(mut self, settings: SurfaceSettings) -> Self {
        self.surface = settings;
        self
    }
    /// Caller-selected native attributes, preserving access to winit platform options.
    #[must_use]
    pub fn native_attributes(mut self, attributes: WindowAttributes) -> Self {
        self.attributes = attributes;
        self
    }
    fn validate(&self) -> Result<(), ApplicationError> {
        let invalid_size = match self.attributes.inner_size {
            Some(Size::Logical(size)) => {
                !size.width.is_finite()
                    || !size.height.is_finite()
                    || size.width <= 0.
                    || size.height <= 0.
            }
            Some(Size::Physical(size)) => size.width == 0 || size.height == 0,
            None => false,
        };
        if invalid_size
            || self
                .background
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || self
                .surface
                .surface_options(astrelis_winit::winit::dpi::PhysicalSize::new(1, 1))
                .sample_count
                == 0
        {
            return Err(ApplicationError::InvalidWindowOptions);
        }
        if let Some(theme) = &self.theme {
            theme.validate()?;
        }
        Ok(())
    }
}
/// RXUI window identity, available before native creation and scoped to one runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId {
    runtime: u64,
    serial: u64,
}
struct Life {
    id: WindowId,
    native: RefCell<Option<Arc<Window>>>,
    closing: Cell<bool>,
    alive: Cell<bool>,
    theme: RefCell<Theme>,
    inherits_theme: Cell<bool>,
}
/// Handle to one requested window. Dropping the handle does not close the window.
/// Its state becomes closed on lifecycle removal; the handle retains no native
/// window after close. Explicitly cloned native Arc handles follow winit ownership.
#[derive(Clone)]
pub struct WindowHandle {
    life: Rc<Life>,
}
impl WindowHandle {
    /// Stable identity, independent of the eventual winit identifier.
    pub fn id(&self) -> WindowId {
        self.life.id
    }
    /// Whether close was requested or the native host released this window.
    pub fn is_closed(&self) -> bool {
        !self.life.alive.get() || self.life.closing.get()
    }
    /// Most recently selected effective window theme, also before native creation.
    pub fn theme(&self) -> Theme {
        self.life.theme.borrow().clone()
    }
    /// Native window after creation and before closing, for platform integration.
    pub fn native_window(&self) -> Option<Arc<Window>> {
        if self.is_closed() {
            None
        } else {
            self.life.native.borrow().clone()
        }
    }
}
impl fmt::Debug for WindowHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowHandle")
            .field("id", &self.id())
            .field("closed", &self.is_closed())
            .finish()
    }
}
trait HostedUi {
    fn set_theme(&mut self, theme: Theme) -> Result<bool, UiError>;
    fn mounts(&self) -> Vec<crate::MountId>;
    fn prepare(
        &mut self,
        runtime: &mut Runtime,
        metrics: WindowMetrics,
        painter: &mut UiPainter,
        format: &astrelis::RenderFormat,
    ) -> Result<(), UiError>;
    fn compose(
        &self,
        painter: &mut UiPainter,
        frame: &mut Frame<'_, 'static>,
        scale: f32,
        clear: wgpu::Color,
    ) -> Result<(), UiError>;
    fn prepare_input(
        &mut self,
        runtime: &mut Runtime,
        metrics: WindowMetrics,
        painter: &mut UiPainter,
    ) -> Result<(), UiError>;
    fn pointer(
        &mut self,
        runtime: &mut Runtime,
        event: PointerEvent,
        painter: &mut UiPainter,
        extend: bool,
    ) -> Result<bool, UiError>;
    fn text_input(
        &mut self,
        runtime: &mut Runtime,
        event: TextInputEvent,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError>;
    fn has_text_focus(&self) -> bool;
    fn accepts_text_input(&self) -> bool;
    fn focused_element(&self) -> Option<ElementId>;
    fn ime_reset_revision(&self) -> u64;
    fn selected_text(&self) -> Option<String>;
    fn ime_area(&mut self, painter: &mut UiPainter) -> Result<Option<Bounds>, UiError>;
    fn active(&mut self, active: bool) -> bool;
    fn caret_visible(&mut self, visible: bool);

    fn scroll(&mut self, point: [f32; 2], delta: [f32; 2]) -> Result<bool, UiError>;
    fn focus_next(&mut self, reverse: bool) -> bool;
    fn activate(&mut self, runtime: &mut Runtime) -> Result<bool, UiError>;
    fn invalidate_geometry(&mut self);
    fn dirty(&self, runtime: &Runtime) -> Result<bool, crate::AccessError>;
    fn forget(&self, painter: &mut UiPainter);
    fn accessibility_update(
        &self,
        tree: &mut AccessKitTree,
        title: &str,
        metrics: WindowMetrics,
    ) -> Result<Option<accesskit::TreeUpdate>, UiError>;
    fn semantic_action(
        &mut self,
        runtime: &mut Runtime,
        action: SemanticAction,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError>;
}
impl<T: View> HostedUi for Ui<T> {
    fn set_theme(&mut self, theme: Theme) -> Result<bool, UiError> {
        self.set_theme(theme)
    }
    fn mounts(&self) -> Vec<crate::MountId> {
        self.mount_ids().collect()
    }
    fn prepare(
        &mut self,
        runtime: &mut Runtime,
        metrics: WindowMetrics,
        painter: &mut UiPainter,
        format: &astrelis::RenderFormat,
    ) -> Result<(), UiError> {
        let size = metrics.logical_size();
        self.prepare(runtime, [size.width as f32, size.height as f32], painter)?;
        painter.prepare(self, format, metrics.scale_factor() as f32)
    }
    fn compose(
        &self,
        painter: &mut UiPainter,
        frame: &mut Frame<'_, 'static>,
        scale: f32,
        clear: wgpu::Color,
    ) -> Result<(), UiError> {
        painter.compose(self, frame, scale, |frame, ui| {
            let mut pass = frame.render_pass().clear_color(clear).begin()?;
            ui.paint(&mut pass)
        })
    }
    fn prepare_input(
        &mut self,
        runtime: &mut Runtime,
        metrics: WindowMetrics,
        painter: &mut UiPainter,
    ) -> Result<(), UiError> {
        if self.needs_prepare(runtime)? || self.measurement_generation() != painter.generation() {
            let size = metrics.logical_size();
            self.prepare(runtime, [size.width as f32, size.height as f32], painter)?;
        }
        Ok(())
    }
    fn pointer(
        &mut self,
        runtime: &mut Runtime,
        event: PointerEvent,
        painter: &mut UiPainter,
        extend: bool,
    ) -> Result<bool, UiError> {
        self.pointer_with_text(runtime, event, painter, extend)
    }
    fn text_input(
        &mut self,
        runtime: &mut Runtime,
        event: TextInputEvent,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError> {
        self.text_input(runtime, event, painter)
    }
    fn has_text_focus(&self) -> bool {
        self.has_text_focus()
    }
    fn accepts_text_input(&self) -> bool {
        self.accepts_text_input()
    }
    fn focused_element(&self) -> Option<ElementId> {
        self.focused_element()
    }
    fn ime_reset_revision(&self) -> u64 {
        self.ime_reset_revision()
    }
    fn selected_text(&self) -> Option<String> {
        self.selected_text().map(str::to_owned)
    }
    fn ime_area(&mut self, painter: &mut UiPainter) -> Result<Option<Bounds>, UiError> {
        self.ime_cursor_area(painter)
    }
    fn active(&mut self, active: bool) -> bool {
        self.set_active(active)
    }
    fn caret_visible(&mut self, visible: bool) {
        self.set_caret_visible(visible);
    }

    fn scroll(&mut self, point: [f32; 2], delta: [f32; 2]) -> Result<bool, UiError> {
        self.scroll(point, delta)
    }
    fn focus_next(&mut self, reverse: bool) -> bool {
        self.focus_next(reverse)
    }
    fn activate(&mut self, runtime: &mut Runtime) -> Result<bool, UiError> {
        self.activate_focused(runtime)
    }
    fn invalidate_geometry(&mut self) {
        self.invalidate_geometry();
    }
    fn dirty(&self, runtime: &Runtime) -> Result<bool, crate::AccessError> {
        self.needs_prepare(runtime)
    }
    fn forget(&self, painter: &mut UiPainter) {
        painter.forget(self);
    }
    fn accessibility_update(
        &self,
        tree: &mut AccessKitTree,
        title: &str,
        metrics: WindowMetrics,
    ) -> Result<Option<accesskit::TreeUpdate>, UiError> {
        tree.update(self, title, metrics.scale_factor() as f32)
    }
    fn semantic_action(
        &mut self,
        runtime: &mut Runtime,
        action: SemanticAction,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError> {
        self.semantic_action(runtime, action, painter)
    }
}
type Factory = dyn FnOnce(&mut Runtime) -> Result<Box<dyn HostedUi>, UiError>;
enum Command {
    Open {
        life: Rc<Life>,
        options: Box<WindowOptions>,
        factory: Box<Factory>,
    },
    Close(WindowId),
    Redraw(WindowId),
    Theme(WindowId),
    Exit,
}
pub(crate) struct Commands {
    runtime: u64,
    next: Cell<u64>,
    queue: RefCell<VecDeque<Command>>,
    lives: RefCell<HashMap<WindowId, Rc<Life>>>,
    exited: Cell<bool>,
    mounts: RefCell<HashMap<crate::MountId, WindowId>>,
    theme: RefCell<Theme>,
}
impl Drop for Commands {
    fn drop(&mut self) {
        for life in self.lives.get_mut().values() {
            life.alive.set(false);
            life.native.borrow_mut().take();
        }
    }
}
impl Commands {
    fn new(runtime: u64) -> Self {
        Self {
            runtime,
            next: Cell::new(1),
            queue: RefCell::new(VecDeque::new()),
            lives: RefCell::new(HashMap::new()),
            exited: Cell::new(false),
            mounts: RefCell::new(HashMap::new()),
            theme: RefCell::new(Theme::default()),
        }
    }
    fn check(&self) -> Result<(), ApplicationError> {
        if self.exited.get() {
            Err(ApplicationError::Exited)
        } else {
            Ok(())
        }
    }
}
impl AppContext<'_> {
    /// Window that dispatched the current listener, also in nested entity updates.
    /// Initialization, ordinary updates and entity-scoped task completions have no
    /// implicit source window. Shared model identity does not select a window.
    pub fn window(&self) -> Option<WindowHandle> {
        let mount = self.dispatch_mount?;
        let commands = self.runtime.native.borrow().clone()?;
        if commands.exited.get() {
            return None;
        }
        let id = *commands.mounts.borrow().get(&mount)?;
        let life = commands.lives.borrow().get(&id)?.clone();
        let handle = WindowHandle { life };
        if handle.is_closed() {
            None
        } else {
            Some(handle)
        }
    }
    fn native_commands(&self) -> Result<Rc<Commands>, ApplicationError> {
        let commands = self
            .runtime
            .native
            .borrow()
            .clone()
            .ok_or(ApplicationError::NoHost)?;
        commands.check()?;
        Ok(commands)
    }
    /// Queues a root view window, retaining the entity until creation or cancellation.
    /// Its native handle becomes available in Application's window_created hook.
    pub fn open_window<T: View>(
        &mut self,
        options: WindowOptions,
        root: Entity<T>,
    ) -> Result<WindowHandle, ApplicationError> {
        self.validate(root.id())?;
        options.validate()?;
        let commands = self.native_commands()?;
        let serial = commands.next.get();
        commands.next.set(
            serial
                .checked_add(1)
                .expect("RXUI window identity exhausted"),
        );
        let life = Rc::new(Life {
            id: WindowId {
                runtime: commands.runtime,
                serial,
            },
            native: RefCell::new(None),
            closing: Cell::new(false),
            alive: Cell::new(true),
            theme: RefCell::new(
                options
                    .theme
                    .clone()
                    .unwrap_or_else(|| commands.theme.borrow().clone()),
            ),
            inherits_theme: Cell::new(options.theme.is_none()),
        });
        commands.lives.borrow_mut().insert(life.id, life.clone());
        commands.queue.borrow_mut().push_back(Command::Open {
            life: life.clone(),
            options: Box::new(options),
            factory: Box::new(move |runtime| Ok(Box::new(Ui::new(runtime, root)?))),
        });
        Ok(WindowHandle { life })
    }
    /// Current application theme. WindowHandle::theme includes any window override.
    pub fn theme(&self) -> Result<Theme, ApplicationError> {
        Ok(self.native_commands()?.theme.borrow().clone())
    }
    /// Changes the application theme and queues updates for inheriting windows.
    /// Explicit window/subtree themes and literal styling remain unchanged.
    pub fn set_theme(&mut self, theme: Theme) -> Result<(), ApplicationError> {
        theme.validate()?;
        let commands = self.native_commands()?;
        if *commands.theme.borrow() == theme {
            return Ok(());
        }
        *commands.theme.borrow_mut() = theme.clone();
        for life in commands.lives.borrow().values() {
            if life.inherits_theme.get() && life.alive.get() && !life.closing.get() {
                *life.theme.borrow_mut() = theme.clone();
                commands
                    .queue
                    .borrow_mut()
                    .push_back(Command::Theme(life.id));
            }
        }
        Ok(())
    }
    /// Sets one window's explicit theme. Creation may still be queued; updates take
    /// effect after the current scope without resetting that window's input state.
    pub fn set_window_theme(
        &mut self,
        window: &WindowHandle,
        theme: Theme,
    ) -> Result<(), ApplicationError> {
        theme.validate()?;
        self.change_window_theme(window, Some(theme))
    }
    /// Restores a window's live inheritance from the application theme.
    pub fn use_application_theme(&mut self, window: &WindowHandle) -> Result<(), ApplicationError> {
        self.change_window_theme(window, None)
    }
    fn change_window_theme(
        &mut self,
        window: &WindowHandle,
        theme: Option<Theme>,
    ) -> Result<(), ApplicationError> {
        let commands = self.native_commands()?;
        if window.id().runtime != commands.runtime {
            return Err(crate::AccessError::WrongRuntime.into());
        }
        if window.is_closed() {
            return Err(ApplicationError::ClosedWindow);
        }
        window.life.inherits_theme.set(theme.is_none());
        let theme = theme.unwrap_or_else(|| commands.theme.borrow().clone());
        if *window.life.theme.borrow() != theme {
            *window.life.theme.borrow_mut() = theme;
            commands
                .queue
                .borrow_mut()
                .push_back(Command::Theme(window.id()));
        }
        Ok(())
    }
    /// Requests close, idempotently while already closing. No acquisition follows a
    /// queued close; shared entities/jobs survive if other strong owners remain.
    pub fn close_window(&mut self, window: &WindowHandle) -> Result<(), ApplicationError> {
        let commands = self.native_commands()?;
        if window.id().runtime != commands.runtime {
            return Err(crate::AccessError::WrongRuntime.into());
        }
        if !window.life.alive.get() {
            return Err(ApplicationError::ClosedWindow);
        }
        if !window.life.closing.replace(true) {
            commands
                .queue
                .borrow_mut()
                .push_back(Command::Close(window.id()));
        }
        Ok(())
    }
    /// Explicit visual invalidation for native/custom integration changes.
    pub fn request_redraw(&mut self, window: &WindowHandle) -> Result<(), ApplicationError> {
        let commands = self.native_commands()?;
        if window.id().runtime != commands.runtime {
            return Err(crate::AccessError::WrongRuntime.into());
        }
        if window.is_closed() {
            return Err(ApplicationError::ClosedWindow);
        }
        commands
            .queue
            .borrow_mut()
            .push_back(Command::Redraw(window.id()));
        Ok(())
    }
    /// Requests orderly application shutdown after the current synchronous scope.
    pub fn exit(&mut self) -> Result<(), ApplicationError> {
        let commands = self.native_commands()?;
        commands.queue.borrow_mut().push_back(Command::Exit);
        commands.exited.set(true);
        Ok(())
    }
}
fn clipboard(
    slot: &mut Option<arboard::Clipboard>,
) -> Result<&mut arboard::Clipboard, arboard::Error> {
    if slot.is_none() {
        *slot = Some(arboard::Clipboard::new()?);
    }
    Ok(slot.as_mut().unwrap())
}
/// Graphics preparation data before surface acquisition. Hooks may create/resize
/// resources and update models; no entity borrow or render pass spans this call.
pub struct GraphicsPrepareContext<'a> {
    /// Explicit source window; AppContext has no implicit listener source here.
    pub window: &'a WindowHandle,
    /// Shared device used by the window and UiPainter.
    pub graphics: &'a GraphicsContext,
    /// Logical/physical size and DPI for backing-resolution choices.
    pub metrics: WindowMetrics,
    /// Surface attachment format for custom pipeline preparation.
    pub format: &'a astrelis::RenderFormat,
}
type GraphicsPrepareHook =
    dyn FnMut(GraphicsPrepareContext<'_>, &mut AppContext<'_>) -> Result<(), ApplicationError>;
type GraphicsRenderHook = dyn FnMut(
    &WindowHandle,
    WindowInfo<'_>,
    &mut Frame<'_, 'static>,
) -> Result<(), ApplicationError>;
type CreatedHook = dyn FnMut(&WindowHandle, &mut AppContext<'_>);
type CloseHook = dyn FnMut(&WindowHandle, &mut AppContext<'_>) -> astrelis_winit::CloseResponse;
type ExitHook = dyn FnOnce(&mut AppContext<'_>);
/// Desktop host over astrelis-winit. It owns native wiring and on-demand scheduling;
/// Runtime/Ui/UiPainter remain usable in custom hosts. Initialization runs once,
/// model/task progression continues independently of surface availability.
pub struct Application {
    theme: Theme,
    fonts: Vec<Arc<[u8]>>,
    system_fonts: bool,
    accessibility: bool,
    executor: Option<Arc<dyn TaskExecutor>>,
    graphics: Option<GraphicsContext>,
    runner_options: RunnerOptions,
    prepare_graphics: Option<Box<GraphicsPrepareHook>>,
    render_graphics: Option<Box<GraphicsRenderHook>>,
    created: Option<Box<CreatedHook>>,
    close: Option<Box<CloseHook>>,
    exiting: Option<Box<ExitHook>>,
}
impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}
impl Application {
    /// Defaults to on-demand windows, explicit host-selected system font discovery,
    /// two async workers and two separate blocking workers, created only at run.
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
            fonts: Vec::new(),
            system_fonts: true,
            accessibility: true,
            executor: None,
            graphics: None,
            runner_options: RunnerOptions::default(),
            prepare_graphics: None,
            render_graphics: None,
            created: None,
            close: None,
            exiting: None,
        }
    }
    /// Default theme for windows without an explicit WindowOptions theme.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
    /// Runs GPU resource preparation before acquisition and before UiPainter's GPU
    /// preparation. It may run on retries without a presented frame. Model updates
    /// are flushed/reconciled afterward; use the supplied window as event source.
    #[must_use]
    pub fn prepare_graphics(
        mut self,
        hook: impl FnMut(
            GraphicsPrepareContext<'_>,
            &mut AppContext<'_>,
        ) -> Result<(), ApplicationError>
        + 'static,
    ) -> Self {
        self.prepare_graphics = Some(Box::new(hook));
        self
    }
    /// Records application GPU work before the UI pass in the same frame/encoder.
    /// Prepare/resize sources in prepare_graphics; this hook gets no mutable model
    /// context so recording cannot invalidate the prepared UI. It must not finish
    /// the host frame or sample a texture while writing it in the same pass.
    #[must_use]
    pub fn render_graphics(
        mut self,
        hook: impl FnMut(
            &WindowHandle,
            WindowInfo<'_>,
            &mut Frame<'_, 'static>,
        ) -> Result<(), ApplicationError>
        + 'static,
    ) -> Self {
        self.render_graphics = Some(Box::new(hook));
        self
    }
    /// Adds application font data, selecting it instead of default system discovery.
    #[must_use]
    pub fn font(mut self, bytes: impl Into<Arc<[u8]>>) -> Self {
        self.fonts.push(bytes.into());
        self.system_fonts = false;
        self
    }
    /// Explicitly enables/disables discovery alongside application-provided fonts.
    #[must_use]
    pub fn system_fonts(mut self, enabled: bool) -> Self {
        self.system_fonts = enabled;
        self
    }
    /// Installs native AccessKit adapters before windows are shown (default true).
    /// Set false when a custom window_created hook owns accessibility integration.
    /// Tree publication remains lazy until assistive technology activates a window.
    #[must_use]
    pub fn accessibility(mut self, enabled: bool) -> Self {
        self.accessibility = enabled;
        self
    }
    /// Uses a caller-owned execution/service-runtime adapter.
    #[must_use]
    pub fn executor(mut self, executor: Arc<dyn TaskExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }
    /// Uses a caller-selected graphics device for all managed windows.
    #[must_use]
    pub fn graphics(mut self, graphics: GraphicsContext) -> Self {
        self.graphics = Some(graphics);
        self
    }
    /// Native runner lifecycle/retry/exit policy customization.
    #[must_use]
    pub fn runner_options(mut self, options: RunnerOptions) -> Self {
        self.runner_options = options;
        self
    }
    /// Runs after native creation and before showing each managed window. Native
    /// adapter setup can access WindowHandle::native_window without owning the loop.
    #[must_use]
    pub fn window_created(
        mut self,
        hook: impl FnMut(&WindowHandle, &mut AppContext<'_>) + 'static,
    ) -> Self {
        self.created = Some(Box::new(hook));
        self
    }
    /// Vetoes or accepts OS close requests, for example while saving asynchronously.
    /// Explicit cx.close_window is an already-decided close and bypasses this hook.
    #[must_use]
    pub fn close_requested(
        mut self,
        hook: impl FnMut(&WindowHandle, &mut AppContext<'_>) -> astrelis_winit::CloseResponse + 'static,
    ) -> Self {
        self.close = Some(Box::new(hook));
        self
    }
    /// Final synchronous application cleanup with a valid runtime context. Complete
    /// async shutdown work before accepting close; remaining jobs cancel at runtime drop.
    #[must_use]
    pub fn exiting(mut self, hook: impl FnOnce(&mut AppContext<'_>) + 'static) -> Self {
        self.exiting = Some(Box::new(hook));
        self
    }
    /// Runs the native loop and invokes initialization once on first resume.
    /// Effects/window commands finish outside active entity update scopes.
    pub fn run(
        self,
        initialize: impl FnOnce(&mut AppContext<'_>) -> Result<(), ApplicationError>,
    ) -> Result<(), ApplicationError> {
        self.theme.validate()?;
        let mut runner = Runner::new()
            .map_err(|e| ApplicationError::Native(Box::new(e)))?
            .with_options(self.runner_options);
        if let Some(graphics) = self.graphics {
            runner = runner.with_graphics(graphics);
        }
        let proxy = runner.proxy();
        let mut runtime = Runtime::new();
        let executor = match self.executor {
            Some(executor) => executor,
            None => Arc::new(
                ThreadPoolExecutor::new(2, 2).map_err(|e| ApplicationError::Native(Box::new(e)))?,
            ),
        };
        runtime.configure_tasks(executor, move || {
            let _ = proxy.send_event(Wake::Tasks);
        })?;
        let commands = Rc::new(Commands::new(runtime.inner.id));
        *commands.theme.borrow_mut() = self.theme;
        *runtime.inner.native.borrow_mut() = Some(commands.clone());
        let mut host = Host {
            runtime,
            commands,
            initialize: Some(initialize),
            windows: HashMap::new(),
            painter: None,
            fonts: self.fonts,
            system_fonts: self.system_fonts,
            active: false,
            prepare_graphics: self.prepare_graphics,
            render_graphics: self.render_graphics,
            created: self.created,
            close: self.close,
            exiting: self.exiting,
            clipboard: None,
            accessibility_enabled: self.accessibility,
        };
        runner
            .run(&mut host)
            .map_err(|e| ApplicationError::Native(Box::new(e)))
    }
}
enum Wake {
    Tasks,
    Accessibility(accesskit_winit::Event),
}
impl From<accesskit_winit::Event> for Wake {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}
struct HostedWindow {
    life: Rc<Life>,
    ui: Option<Box<dyn HostedUi>>,
    factory: Option<Box<Factory>>,
    background: Option<Color>,
    cursor: [f64; 2],
    modifiers: ModifiersState,
    blink_at: Instant,
    caret_visible: bool,
    ime_focus: Option<ElementId>,
    ime_reset_revision: u64,
    ime_allowed: bool,
    ime_area: Option<Bounds>,
    accessibility: Option<accesskit_winit::Adapter>,
    accesskit: AccessKitTree,
    accessibility_active: bool,
}
struct Host<F> {
    runtime: Runtime,
    commands: Rc<Commands>,
    initialize: Option<F>,
    windows: HashMap<NativeWindowId, HostedWindow>,
    painter: Option<UiPainter>,
    fonts: Vec<Arc<[u8]>>,
    system_fonts: bool,
    active: bool,
    prepare_graphics: Option<Box<GraphicsPrepareHook>>,
    render_graphics: Option<Box<GraphicsRenderHook>>,
    created: Option<Box<CreatedHook>>,
    close: Option<Box<CloseHook>>,
    exiting: Option<Box<ExitHook>>,
    clipboard: Option<arboard::Clipboard>,
    accessibility_enabled: bool,
}
impl<F> Drop for Host<F> {
    fn drop(&mut self) {
        // Platform adapters release hooks while their native windows still exist,
        // including run failures that bypass the normal exiting callback.
        for (_, mut window) in self.windows.drain() {
            drop(window.accessibility.take());
            window.life.alive.set(false);
            window.life.native.borrow_mut().take();
        }
    }
}
impl<F> Host<F> {
    fn sync_mounts(&self, id: NativeWindowId) {
        let window = &self.windows[&id];
        let Some(ui) = &window.ui else {
            return;
        };
        let ids: HashSet<_> = ui.mounts().into_iter().collect();
        let mut mounts = self.commands.mounts.borrow_mut();
        mounts.retain(|mount, owner| *owner != window.life.id || ids.contains(mount));
        for mount in ids {
            mounts.insert(mount, window.life.id);
        }
    }
    fn sync_text_platform(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<(), ApplicationError> {
        let Some(native) = cx.window(id) else {
            return Ok(());
        };
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        let Some(ui) = &mut window.ui else {
            return Ok(());
        };
        let focus = ui
            .accepts_text_input()
            .then(|| ui.focused_element())
            .flatten();
        let reset_revision = ui.ime_reset_revision();
        if focus != window.ime_focus || reset_revision != window.ime_reset_revision {
            window.ime_reset_revision = reset_revision;
            if window.ime_allowed {
                native.window().set_ime_allowed(false);
            }
            window.ime_focus = focus;
            window.ime_allowed = focus.is_some();
            native.window().set_ime_allowed(window.ime_allowed);
            window.ime_area = None;
        }
        if window.ime_allowed
            && let Some(painter) = &mut self.painter
            && let Some(area) = ui.ime_area(painter)?
            && window.ime_area != Some(area)
        {
            native.window().set_ime_cursor_area(
                LogicalPosition::new(area.x as f64, area.y as f64),
                LogicalSize::new(area.width as f64, area.height as f64),
            );
            window.ime_area = Some(area);
        }
        if ui.has_text_focus() {
            cx.request_redraw_at(id, window.blink_at)
                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
        } else {
            cx.cancel_redraw_at(id)
                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
        }
        Ok(())
    }
    fn publish_accessibility(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<(), ApplicationError> {
        let Some(native) = cx.window(id) else {
            return Ok(());
        };
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        if !window.accessibility_active || window.life.closing.get() {
            return Ok(());
        }
        let (Some(ui), Some(adapter), Some(painter)) =
            (&mut window.ui, &mut window.accessibility, &mut self.painter)
        else {
            return Ok(());
        };
        // CPU descriptions/layout/semantics progress even if a surface cannot render.
        ui.prepare_input(&mut self.runtime, native.metrics(), painter)?;
        if let Some(update) = ui.accessibility_update(
            &mut window.accesskit,
            &native.window().title(),
            native.metrics(),
        )? {
            adapter.update_if_active(|| update);
        }
        self.sync_mounts(id);
        Ok(())
    }
    fn progress(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        preparing: Option<NativeWindowId>,
    ) -> Result<(), ApplicationError> {
        if cx.event_loop().exiting() {
            return Ok(());
        }
        if !self.commands.exited.get() {
            self.runtime.poll_tasks();
            self.runtime.flush()?;
        }
        let pending = std::mem::take(&mut *self.commands.queue.borrow_mut());
        for command in pending {
            match command {
                Command::Open {
                    life,
                    options,
                    factory,
                } => {
                    if life.closing.get() || self.commands.exited.get() {
                        life.alive.set(false);
                        self.commands.lives.borrow_mut().remove(&life.id);
                        continue;
                    }
                    if !self.active {
                        self.commands.queue.borrow_mut().push_back(Command::Open {
                            life,
                            options,
                            factory,
                        });
                        continue;
                    }
                    let native = cx
                        .create_window(options.attributes, options.surface)
                        .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                    self.windows.insert(
                        native,
                        HostedWindow {
                            life,
                            ui: None,
                            factory: Some(factory),
                            background: options.background,
                            cursor: [0.; 2],
                            modifiers: ModifiersState::default(),
                            blink_at: Instant::now() + Duration::from_millis(500),
                            caret_visible: true,
                            ime_focus: None,
                            ime_reset_revision: 0,
                            ime_allowed: false,
                            ime_area: None,
                            accessibility: None,
                            accesskit: AccessKitTree::new(),
                            accessibility_active: false,
                        },
                    );
                }
                Command::Close(id) => {
                    if let Some(native) = self
                        .windows
                        .iter()
                        .find(|(_, window)| window.life.id == id)
                        .map(|(id, _)| *id)
                    {
                        cx.close_window(native)
                            .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                    }
                }
                Command::Redraw(id) => {
                    if let Some(native) = self
                        .windows
                        .iter()
                        .find(|(_, window)| window.life.id == id)
                        .map(|(id, _)| *id)
                        && cx.window(native).is_some()
                    {
                        cx.request_redraw(native)
                            .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                    }
                }
                Command::Theme(id) => {
                    if let Some((native, window)) =
                        self.windows.iter_mut().find(|(_, w)| w.life.id == id)
                    {
                        if window.life.closing.get() {
                            continue;
                        }
                        if let Some(ui) = &mut window.ui {
                            ui.set_theme(window.life.theme.borrow().clone())?;
                        }
                        if cx.window(*native).is_some() {
                            cx.request_redraw(*native)
                                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                        }
                    }
                }
                Command::Exit => {
                    cx.exit();
                    self.commands.exited.set(true);
                    break;
                }
            }
        }
        if cx.event_loop().exiting() {
            return Ok(());
        }
        // Queue visual work before accessibility preparation consumes component
        // dirtiness. Semantic publication may prepare CPU snapshots for every active
        // adapter; it must not erase another window's shared-model redraw request.
        for (id, window) in &self.windows {
            if Some(*id) != preparing
                && !window.life.closing.get()
                && window
                    .ui
                    .as_ref()
                    .is_some_and(|ui| ui.dirty(&self.runtime).unwrap_or(true))
                && cx.window(*id).is_some()
            {
                cx.request_redraw(*id)
                    .map_err(|e| ApplicationError::Native(Box::new(e)))?;
            }
        }
        let accessible: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| w.accessibility_active && !w.life.closing.get())
            .map(|(id, _)| *id)
            .collect();
        for id in accessible {
            self.publish_accessibility(cx, id)?;
        }
        Ok(())
    }
}

impl<F: FnOnce(&mut AppContext<'_>) -> Result<(), ApplicationError>> Handler for Host<F> {
    type Message = Wake;
    type Error = ApplicationError;
    fn resumed(&mut self, cx: &mut NativeContext<'_, Wake>) -> Result<(), Self::Error> {
        self.active = true;
        if let Some(initialize) = self.initialize.take() {
            self.runtime.update(initialize)?;
        }
        self.progress(cx, None)
    }
    fn suspended(&mut self, cx: &mut NativeContext<'_, Wake>) -> Result<(), Self::Error> {
        self.active = false;
        self.progress(cx, None)
    }
    fn window_created(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<(), Self::Error> {
        if self.painter.is_none() {
            let mut painter = UiPainter::new(cx.window(id).unwrap().graphics());
            if self.system_fonts {
                painter.fonts_mut().load_system_fonts();
            }
            for bytes in &self.fonts {
                painter
                    .fonts_mut()
                    .load_font_shared(bytes.clone())
                    .map_err(UiError::from)?;
            }
            self.painter = Some(painter);
        }
        let window = self.windows.get_mut(&id).unwrap();
        *window.life.native.borrow_mut() = Some(cx.window(id).unwrap().window().clone());
        window.ui = Some(window.factory.take().unwrap()(&mut self.runtime)?);
        window
            .ui
            .as_mut()
            .unwrap()
            .set_theme(window.life.theme.borrow().clone())?;
        if self.accessibility_enabled {
            window.accessibility = Some(accesskit_winit::Adapter::with_event_loop_proxy(
                cx.event_loop(),
                cx.window(id).unwrap().window(),
                cx.proxy(),
            ));
        }
        if let Some(hook) = &mut self.created {
            let handle = WindowHandle {
                life: window.life.clone(),
            };
            self.runtime.update(|cx| hook(&handle, cx));
        }
        self.sync_mounts(id);
        self.progress(cx, None)
    }
    fn window_event(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        if let Some(window) = self.windows.get_mut(&id)
            && let Some(adapter) = &mut window.accessibility
            && let Some(native) = window.life.native.borrow().as_ref()
        {
            adapter.process_event(native, &event);
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            return Ok(());
        }
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        let Some(ui) = &mut window.ui else {
            return Ok(());
        };
        let mut changed = false;
        let Some(native) = cx.window(id) else {
            return Ok(());
        };
        let scale = native.metrics().scale_factor();
        let Some(painter) = &mut self.painter else {
            return Ok(());
        };
        ui.prepare_input(&mut self.runtime, native.metrics(), painter)?;
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                ui.invalidate_geometry()
            }
            WindowEvent::CursorMoved { position, .. } => {
                window.cursor = [position.x, position.y];
                changed = ui.pointer(
                    &mut self.runtime,
                    PointerEvent::Moved([(position.x / scale) as f32, (position.y / scale) as f32]),
                    painter,
                    window.modifiers.shift_key(),
                )?;
            }
            WindowEvent::CursorLeft { .. } => {
                changed = ui.pointer(&mut self.runtime, PointerEvent::Left, painter, false)?
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let point = [
                    (window.cursor[0] / scale) as f32,
                    (window.cursor[1] / scale) as f32,
                ];
                changed = ui.pointer(
                    &mut self.runtime,
                    if state == ElementState::Pressed {
                        PointerEvent::Pressed(point)
                    } else {
                        PointerEvent::Released(point)
                    },
                    painter,
                    window.modifiers.shift_key(),
                )?;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let motion = match delta {
                    MouseScrollDelta::LineDelta(x, y) => [-x * 40., -y * 40.],
                    MouseScrollDelta::PixelDelta(p) => {
                        [-(p.x / scale) as f32, -(p.y / scale) as f32]
                    }
                };
                changed = ui.scroll(
                    [
                        (window.cursor[0] / scale) as f32,
                        (window.cursor[1] / scale) as f32,
                    ],
                    motion,
                )?;
            }
            WindowEvent::Focused(active) => {
                changed = ui.active(active);
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                changed = ui.text_input(
                    &mut self.runtime,
                    TextInputEvent::Preedit { text, cursor },
                    painter,
                )?;
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                changed =
                    ui.text_input(&mut self.runtime, TextInputEvent::Commit(text), painter)?;
            }
            WindowEvent::Ime(Ime::Disabled) => {
                changed = ui.text_input(
                    &mut self.runtime,
                    TextInputEvent::CancelComposition,
                    painter,
                )?;
            }
            WindowEvent::ModifiersChanged(modifiers) => window.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed => {
                let modifiers = window.modifiers;
                let primary = if cfg!(target_os = "macos") {
                    modifiers.super_key()
                } else {
                    modifiers.control_key() && !modifiers.alt_key()
                };
                let word = if cfg!(target_os = "macos") {
                    modifiers.alt_key()
                } else {
                    modifiers.control_key()
                };
                let extend = modifiers.shift_key();
                if event.logical_key == Key::Named(NamedKey::Tab) && !event.repeat {
                    changed = ui.focus_next(extend);
                } else if ui.has_text_focus() {
                    let input = match &event.logical_key {
                        Key::Character(key) if primary && key.eq_ignore_ascii_case("a") => {
                            Some(TextInputEvent::SelectAll)
                        }
                        Key::Character(key)
                            if primary
                                && (key.eq_ignore_ascii_case("c")
                                    || key.eq_ignore_ascii_case("x")) =>
                        {
                            if let Some(text) = ui.selected_text() {
                                match clipboard(&mut self.clipboard).and_then(|c| c.set_text(text))
                                {
                                    Ok(())
                                        if key.eq_ignore_ascii_case("x")
                                            && ui.accepts_text_input() =>
                                    {
                                        changed = ui.text_input(
                                            &mut self.runtime,
                                            TextInputEvent::Insert(String::new()),
                                            painter,
                                        )?;
                                    }
                                    Err(error) => eprintln!("RXUI clipboard: {error}"),
                                    _ => {}
                                }
                            }
                            None
                        }
                        Key::Character(key) if primary && key.eq_ignore_ascii_case("v") => {
                            match clipboard(&mut self.clipboard).and_then(|c| c.get_text()) {
                                Ok(text) => Some(TextInputEvent::Insert(text)),
                                Err(error) => {
                                    eprintln!("RXUI clipboard: {error}");
                                    None
                                }
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                            let right = event.logical_key == Key::Named(NamedKey::ArrowRight);
                            let movement = if cfg!(target_os = "macos") && primary {
                                if right {
                                    TextMovement::End
                                } else {
                                    TextMovement::Start
                                }
                            } else if word {
                                if right {
                                    TextMovement::WordRight
                                } else {
                                    TextMovement::WordLeft
                                }
                            } else if right {
                                TextMovement::Right
                            } else {
                                TextMovement::Left
                            };
                            Some(TextInputEvent::Move { movement, extend })
                        }
                        Key::Named(NamedKey::Home) => Some(TextInputEvent::Move {
                            movement: TextMovement::Start,
                            extend,
                        }),
                        Key::Named(NamedKey::End) => Some(TextInputEvent::Move {
                            movement: TextMovement::End,
                            extend,
                        }),
                        Key::Named(NamedKey::Backspace) => Some(TextInputEvent::Backspace),
                        Key::Named(NamedKey::Delete) => Some(TextInputEvent::Delete),
                        Key::Named(NamedKey::Escape) => Some(TextInputEvent::CancelComposition),
                        Key::Named(NamedKey::Enter) if !event.repeat => {
                            Some(TextInputEvent::Submit)
                        }
                        _ if !primary && (!modifiers.control_key() || modifiers.alt_key()) => event
                            .text
                            .as_ref()
                            .filter(|text| text.chars().any(|c| !c.is_control()))
                            .map(|text| TextInputEvent::Insert(text.to_string())),
                        _ => None,
                    };
                    if let Some(input) = input {
                        changed |= ui.text_input(&mut self.runtime, input, painter)?;
                    }
                } else if !event.repeat
                    && matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Enter | NamedKey::Space)
                    )
                {
                    changed = ui.activate(&mut self.runtime)?;
                }
            }
            _ => {}
        }
        if changed {
            window.caret_visible = true;
            window.blink_at = Instant::now() + Duration::from_millis(500);
            ui.caret_visible(true);
            cx.cancel_redraw_at(id)
                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
            cx.request_redraw(id)
                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
        }
        self.sync_text_platform(cx, id)?;
        self.progress(cx, None)
    }
    fn user_event(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        event: Wake,
    ) -> Result<(), Self::Error> {
        match event {
            Wake::Tasks => self.progress(cx, None),
            Wake::Accessibility(event) => {
                let id = event.window_id;
                if !self.windows.contains_key(&id) || cx.window(id).is_none() {
                    return Ok(());
                }
                match event.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => {
                        let window = self.windows.get_mut(&id).unwrap();
                        window.accessibility_active = true;
                        window.accesskit.reset();
                        self.progress(cx, None)
                    }
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                        let window = self.windows.get_mut(&id).unwrap();
                        window.accessibility_active = false;
                        window.accesskit.deactivate();
                        self.progress(cx, None)
                    }
                    accesskit_winit::WindowEvent::ActionRequested(request) => {
                        // Deliver model completions first, so decoding or live revision
                        // validation rejects stale requests against replaced values.
                        self.progress(cx, None)?;
                        let Some(native) = cx.window(id) else {
                            return Ok(());
                        };
                        let Some(window) = self.windows.get_mut(&id) else {
                            return Ok(());
                        };
                        if window.life.closing.get() || !window.accessibility_active {
                            return Ok(());
                        }
                        let Some(action) = window.accesskit.action(request) else {
                            return Ok(());
                        };
                        let (Some(ui), Some(painter)) = (&mut window.ui, &mut self.painter) else {
                            return Ok(());
                        };
                        ui.prepare_input(&mut self.runtime, native.metrics(), painter)?;
                        if matches!(action, SemanticAction::Focus(_)) {
                            native.window().focus_window();
                        }
                        let changed = ui.semantic_action(&mut self.runtime, action, painter)?;
                        if changed {
                            window.caret_visible = true;
                            window.blink_at = Instant::now() + Duration::from_millis(500);
                            ui.caret_visible(true);
                            cx.cancel_redraw_at(id)
                                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                            cx.request_redraw(id)
                                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                        }
                        self.sync_text_platform(cx, id)?;
                        self.progress(cx, None)
                    }
                }
            }
        }
    }
    fn close_requested(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<astrelis_winit::CloseResponse, Self::Error> {
        let handle = WindowHandle {
            life: self.windows[&id].life.clone(),
        };
        let response = if let Some(hook) = &mut self.close {
            self.runtime.update(|cx| hook(&handle, cx))
        } else {
            astrelis_winit::CloseResponse::Close
        };
        self.progress(cx, None)?;
        Ok(response)
    }
    fn prepare(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<PrepareAction, Self::Error> {
        self.progress(cx, Some(id))?;
        let Some(native) = cx.window(id) else {
            return Ok(PrepareAction::Skip);
        };
        if let Some(hook) = &mut self.prepare_graphics {
            let handle = WindowHandle {
                life: self.windows[&id].life.clone(),
            };
            let graphics = native.graphics().clone();
            let metrics = native.metrics();
            let format = native.render_format().unwrap().clone();
            self.runtime.update(|cx| {
                hook(
                    GraphicsPrepareContext {
                        window: &handle,
                        graphics: &graphics,
                        metrics,
                        format: &format,
                    },
                    cx,
                )
            })?;
            self.progress(cx, Some(id))?;
        }
        let Some(native) = cx.window(id) else {
            return Ok(PrepareAction::Skip);
        };
        let window = self.windows.get_mut(&id).unwrap();
        if window.life.closing.get() {
            return Ok(PrepareAction::Skip);
        }
        if window.ui.as_ref().unwrap().has_text_focus() && Instant::now() >= window.blink_at {
            window.caret_visible = !window.caret_visible;
            window
                .ui
                .as_mut()
                .unwrap()
                .caret_visible(window.caret_visible);
            window.blink_at = Instant::now() + Duration::from_millis(500);
        }
        window.ui.as_mut().unwrap().prepare(
            &mut self.runtime,
            native.metrics(),
            self.painter.as_mut().unwrap(),
            native.render_format().unwrap(),
        )?;
        self.sync_mounts(id);
        self.sync_text_platform(cx, id)?;
        self.publish_accessibility(cx, id)?;
        Ok(PrepareAction::Render)
    }
    fn render(
        &mut self,
        window: WindowInfo<'_>,
        frame: &mut Frame<'_, 'static>,
    ) -> Result<(), Self::Error> {
        if let Some(hook) = &mut self.render_graphics {
            let handle = WindowHandle {
                life: self.windows[&window.id()].life.clone(),
            };
            hook(&handle, window, frame)?;
        }
        let hosted = &self.windows[&window.id()];
        let c = hosted
            .background
            .unwrap_or_else(|| hosted.life.theme.borrow().palette().background);
        hosted.ui.as_ref().unwrap().compose(
            self.painter.as_mut().unwrap(),
            frame,
            window.metrics().scale_factor() as f32,
            wgpu::Color {
                r: c[0] as f64,
                g: c[1] as f64,
                b: c[2] as f64,
                a: c[3] as f64,
            },
        )?;
        Ok(())
    }
    fn window_closed(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<(), Self::Error> {
        if let Some(mut window) = self.windows.remove(&id) {
            drop(window.accessibility.take());
            window.life.alive.set(false);
            window.life.native.borrow_mut().take();
            self.commands.lives.borrow_mut().remove(&window.life.id);
            self.commands
                .mounts
                .borrow_mut()
                .retain(|_, owner| *owner != window.life.id);
            if let Some(ui) = window.ui
                && let Some(painter) = &mut self.painter
            {
                ui.forget(painter);
            }
        }
        self.runtime.synchronize();
        self.progress(cx, None)
    }
    fn exiting(&mut self, _: &mut NativeContext<'_, Wake>) -> Result<(), Self::Error> {
        self.commands.exited.set(true);
        if let Some(hook) = self.exiting.take() {
            self.runtime.update(hook);
            self.runtime.flush()?;
        }
        let pending = std::mem::take(&mut *self.commands.queue.borrow_mut());
        drop(pending);
        for window in self.windows.values_mut() {
            drop(window.accessibility.take());
        }
        for life in self.commands.lives.borrow().values() {
            life.alive.set(false);
            life.native.borrow_mut().take();
        }
        self.runtime.synchronize();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dispatch, IntoElement, ViewContext, label};

    struct Counter(u32);
    impl View for Counter {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            label(self.0.to_string())
        }
    }
    fn attach(runtime: &mut Runtime) -> Rc<Commands> {
        let commands = Rc::new(Commands::new(runtime.inner.id));
        *runtime.inner.native.borrow_mut() = Some(commands.clone());
        commands
    }

    #[test]
    fn window_commands_validate_before_queueing_and_close_is_idempotent() {
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Counter(0)));
        assert!(matches!(
            runtime.update(|cx| cx.open_window(WindowOptions::new(), root.clone())),
            Err(ApplicationError::NoHost)
        ));
        let commands = attach(&mut runtime);
        for options in [
            WindowOptions::new().size(f64::NAN, 20.),
            WindowOptions::new().size(0., 20.),
            WindowOptions::new().background([2.; 4]),
        ] {
            assert!(matches!(
                runtime.update(|cx| cx.open_window(options, root.clone())),
                Err(ApplicationError::InvalidWindowOptions)
            ));
        }
        assert!(commands.queue.borrow().is_empty());
        let handle = runtime
            .update(|cx| cx.open_window(WindowOptions::new(), root.clone()))
            .unwrap();
        assert!(!handle.is_closed());
        assert!(handle.native_window().is_none());
        let weak = root.downgrade();
        drop(root);
        assert!(weak.upgrade().is_some()); // Queued creation owns the root.
        runtime.update(|cx| {
            cx.close_window(&handle).unwrap();
            cx.close_window(&handle).unwrap();
            assert!(matches!(
                cx.request_redraw(&handle),
                Err(ApplicationError::ClosedWindow)
            ));
        });
        assert!(handle.is_closed());
        assert_eq!(commands.queue.borrow().len(), 2); // One open and one close.
        commands.queue.borrow_mut().clear();
        runtime.synchronize();
        assert!(weak.upgrade().is_none());

        let mut foreign = Runtime::new();
        attach(&mut foreign);
        assert!(matches!(
            foreign.update(|cx| cx.close_window(&handle)),
            Err(ApplicationError::Ui(UiError::Access(
                crate::AccessError::WrongRuntime
            )))
        ));
        runtime.update(|cx| cx.exit()).unwrap();
        assert!(matches!(
            runtime.update(|cx| cx.request_redraw(&handle)),
            Err(ApplicationError::Exited)
        ));
    }

    #[test]
    fn theme_changes_cover_pending_windows_inheritance_validation_and_foreign_handles() {
        let mut runtime = Runtime::new();
        let commands = attach(&mut runtime);
        let root = runtime.update(|cx| cx.new(|_| Counter(0)));
        let (inherited, explicit) = runtime.update(|cx| {
            (
                cx.open_window(WindowOptions::new(), root.clone()).unwrap(),
                cx.open_window(WindowOptions::new().theme(Theme::light()), root)
                    .unwrap(),
            )
        });
        runtime.update(|cx| cx.set_theme(Theme::light())).unwrap();
        assert_eq!(inherited.theme(), Theme::light());
        assert_eq!(explicit.theme(), Theme::light());
        runtime.update(|cx| cx.set_theme(Theme::dark())).unwrap();
        assert_eq!(inherited.theme(), Theme::dark());
        assert_eq!(explicit.theme(), Theme::light());
        runtime
            .update(|cx| cx.set_window_theme(&inherited, Theme::light()))
            .unwrap();
        runtime.update(|cx| cx.set_theme(Theme::light())).unwrap();
        runtime.update(|cx| cx.set_theme(Theme::dark())).unwrap();
        assert_eq!(inherited.theme(), Theme::light());
        runtime
            .update(|cx| cx.use_application_theme(&inherited))
            .unwrap();
        assert_eq!(inherited.theme(), Theme::dark());
        let invalid = Theme::dark().metrics(|m| m.font_size = 0.);
        assert!(runtime.update(|cx| cx.set_theme(invalid.clone())).is_err());
        assert!(
            runtime
                .update(|cx| cx.set_window_theme(&explicit, invalid))
                .is_err()
        );
        assert_eq!(*commands.theme.borrow(), Theme::dark());
        assert_eq!(explicit.theme(), Theme::light());
        let mut foreign = Runtime::new();
        attach(&mut foreign);
        assert!(matches!(
            foreign.update(|cx| cx.set_window_theme(&explicit, Theme::dark())),
            Err(ApplicationError::Ui(UiError::Access(
                crate::AccessError::WrongRuntime
            )))
        ));
        runtime.update(|cx| cx.close_window(&inherited)).unwrap();
        assert!(matches!(
            runtime.update(|cx| cx.use_application_theme(&inherited)),
            Err(ApplicationError::ClosedWindow)
        ));
    }

    #[test]
    fn shared_model_listeners_resolve_their_own_window_and_nested_updates_inherit_it() {
        let mut runtime = Runtime::new();
        let commands = attach(&mut runtime);
        let shared = runtime.update(|cx| cx.new(|_| Counter(0)));
        let nested = runtime.update(|cx| cx.new(|_| None));
        let (a, b, ma, mb) = runtime.update(|cx| {
            (
                cx.open_window(WindowOptions::new(), shared.clone())
                    .unwrap(),
                cx.open_window(WindowOptions::new(), shared.clone())
                    .unwrap(),
                cx.mount(&shared).unwrap(),
                cx.mount(&shared).unwrap(),
            )
        });
        commands
            .mounts
            .borrow_mut()
            .extend([(ma.id(), a.id()), (mb.id(), b.id())]);
        let listener = |runtime: &mut Runtime, mount: &crate::Mount<Counter>| {
            runtime
                .evaluate(mount, |_, cx| {
                    cx.listener({
                        let nested = nested.clone();
                        move |model, _: &(), cx| {
                            model.0 += 1;
                            let source = cx.window().unwrap().id();
                            nested.update(cx, |state, cx| {
                                assert_eq!(cx.window().unwrap().id(), source);
                                *state = Some(source);
                            });
                        }
                    })
                })
                .unwrap()
        };
        let la = listener(&mut runtime, &ma);
        let lb = listener(&mut runtime, &mb);
        for (binding, expected) in [(la, a.id()), (lb, b.id())] {
            assert_eq!(
                runtime.update(|cx| binding.dispatch(&(), cx)).unwrap(),
                Dispatch::Handled
            );
            assert_eq!(runtime.update(|cx| *nested.read(cx)), Some(expected));
        }
        assert_eq!(runtime.update(|cx| shared.read(cx).0), 2);
        runtime.update(|cx| {
            assert!(cx.window().is_none());
            shared.update(cx, |_, cx| assert!(cx.window().is_none()));
        });
    }

    #[test]
    fn releasing_host_commands_closes_retained_window_handles() {
        let mut runtime = Runtime::new();
        let commands = attach(&mut runtime);
        let handle = runtime.update(|cx| {
            let root = cx.new(|_| Counter(0));
            cx.open_window(WindowOptions::new(), root).unwrap()
        });
        drop(commands);
        drop(runtime);
        assert!(handle.is_closed());
    }
}
