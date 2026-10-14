use crate::{
    AccessKitTree, AppContext, Bounds, Color, ColorScheme, EffectCycle, ElementId, Entity,
    FocusRingPolicy, PointerEvent, ReadContext, Runtime, SemanticAction, SpawnError,
    SystemPreferences, TaskExecutor, TextInputEvent, TextMeasure, Theme, ThemeMode, ThemeSet,
    ThreadPoolExecutor, Ui, UiError, UiPainter, View,
};
#[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
mod dialogs;
#[cfg(all(
    feature = "native-menus",
    any(target_os = "macos", target_os = "windows")
))]
mod menu_backend;
#[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
pub use dialogs::{
    DialogError, DialogResult, DialogTask, FileDialog, MessageButtons, MessageDialog, MessageLevel,
    MessageResponse,
};
#[cfg(all(feature = "desktop-services", not(target_arch = "wasm32")))]
mod desktop;
use crate::{CommandId, CommandInfo, CommandStatus};
use astrelis::{Frame, GraphicsContext, wgpu};
use astrelis_winit::{
    AppContext as NativeContext, Handler, PrepareAction, Runner, RunnerOptions, SurfaceSettings,
    WindowInfo, WindowMetrics,
    winit::{
        dpi::{LogicalSize, Size},
        event::{MouseButton, WindowEvent},
        keyboard::{Key, ModifiersState, NamedKey},
        window::{Theme as NativeTheme, Window, WindowAttributes, WindowId as NativeWindowId},
    },
};
#[cfg(all(feature = "desktop-services", not(target_arch = "wasm32")))]
pub use desktop::{DesktopError, DesktopResult};
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
    /// A dialog requires an explicit managed parent when no source window exists.
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    NoSourceWindow,
    /// This parent already owns a pending/open native dialog.
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    DialogBusy,
    /// Dialog text/filter options contain invalid native values.
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    InvalidDialogOptions,
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
            #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
            Self::NoSourceWindow => f.write_str("a native dialog requires a managed parent window"),
            #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
            Self::DialogBusy => f.write_str("this window already owns a native dialog"),
            #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
            Self::InvalidDialogOptions => f.write_str("invalid native dialog options"),
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

mod geometry;
pub use geometry::WindowGeometry;
mod preferences;
mod window_input;
pub use window_input::{WindowInput, WindowInputResult};

/// A native file-drag notification for one managed window. Multiple files produce
/// separate events. Paths are OS payloads, not file contents or verified files.
/// Winit supplies no reliable drop position or explicit batch-end notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileDropEvent {
    /// One file is hovering over the window. Some platforms omit hover events.
    Hovered(std::path::PathBuf),
    /// The drag left/cancelled; clear the complete hover preview.
    Cancelled,
    /// One file was dropped. Deliveries may arrive without a preceding hover.
    Dropped(std::path::PathBuf),
}

impl FileDropEvent {
    fn from_window_event(event: &WindowEvent) -> Option<Self> {
        match event {
            WindowEvent::HoveredFile(path) => Some(Self::Hovered(path.clone())),
            WindowEvent::HoveredFileCancelled => Some(Self::Cancelled),
            WindowEvent::DroppedFile(path) => Some(Self::Dropped(path.clone())),
            _ => None,
        }
    }
}
fn deliver_file_drop(
    runtime: &mut Runtime,
    window: &WindowHandle,
    event: &FileDropEvent,
    hook: &mut Option<Box<FileDropHook>>,
) {
    if window.is_closed()
        || runtime
            .inner
            .native
            .borrow()
            .as_ref()
            .is_none_or(|c| c.exited.get())
    {
        return;
    }
    if let Some(hook) = hook {
        runtime.update(|cx| hook(window, event, cx));
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
    restore_geometry: bool,
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
            restore_geometry: false,
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
    /// Restores saved normal bounds and maximized state. Current monitor bounds
    /// clamp oversized/offscreen placement at native creation. Storage stays with
    /// the application; position support follows winit (unavailable on Wayland).
    /// Later size/native_attributes builders override the corresponding attributes.
    #[must_use]
    pub fn geometry(mut self, geometry: WindowGeometry) -> Self {
        self.attributes = geometry.attributes(self.attributes);
        self.restore_geometry = true;
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
    /// Lets the desktop show through unpainted areas. The default clear color
    /// becomes transparent, and the host asks the surface for a non-opaque alpha
    /// mode, falling back to an opaque surface where the graphics backend has none
    /// (DirectX 12 window surfaces, for example). Paint a background where needed.
    #[must_use]
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.attributes = self.attributes.with_transparent(transparent);
        self
    }
    /// Shows or hides the platform title bar and borders (default true). Without
    /// them, draw a title bar and mark it with `Element::window_drag_region`.
    #[must_use]
    pub fn decorations(mut self, decorations: bool) -> Self {
        self.attributes = self.attributes.with_decorations(decorations);
        self
    }
    /// Whether the user can resize the window (default true).
    #[must_use]
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.attributes = self.attributes.with_resizable(resizable);
        self
    }
    /// Smallest logical inner size the user can resize to; finite and positive.
    #[must_use]
    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.attributes = self
            .attributes
            .with_min_inner_size(LogicalSize::new(width, height));
        self
    }
    /// Largest logical inner size the user can resize to; finite, positive and at
    /// least the minimum size.
    #[must_use]
    pub fn max_size(mut self, width: f64, height: f64) -> Self {
        self.attributes = self
            .attributes
            .with_max_inner_size(LogicalSize::new(width, height));
        self
    }
    /// Keeps the window above normal windows, for palettes and floating tools.
    /// Change it later through `WindowHandle::native_window().set_window_level`.
    #[must_use]
    pub fn always_on_top(mut self, on_top: bool) -> Self {
        use astrelis_winit::winit::window::WindowLevel;
        self.attributes = self.attributes.with_window_level(if on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        });
        self
    }
    /// macOS: draws the title bar transparently over the window content. Ignored
    /// on other platforms, so portable code can set it unconditionally.
    #[must_use]
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    pub fn titlebar_transparent(mut self, transparent: bool) -> Self {
        #[cfg(target_os = "macos")]
        {
            use astrelis_winit::winit::platform::macos::WindowAttributesExtMacOS;
            self.attributes = self.attributes.with_titlebar_transparent(transparent);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = transparent;
        self
    }
    /// macOS: extends the content view under the title bar, so the application
    /// draws there and the traffic-light buttons float over it. Ignored elsewhere.
    #[must_use]
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    pub fn fullsize_content_view(mut self, fullsize: bool) -> Self {
        #[cfg(target_os = "macos")]
        {
            use astrelis_winit::winit::platform::macos::WindowAttributesExtMacOS;
            self.attributes = self.attributes.with_fullsize_content_view(fullsize);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = fullsize;
        self
    }
    /// macOS: hides the title text while keeping the title bar and its buttons.
    /// The title is still used by the Window menu and accessibility. Ignored elsewhere.
    #[must_use]
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    pub fn title_hidden(mut self, hidden: bool) -> Self {
        #[cfg(target_os = "macos")]
        {
            use astrelis_winit::winit::platform::macos::WindowAttributesExtMacOS;
            self.attributes = self.attributes.with_title_hidden(hidden);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = hidden;
        self
    }
    /// Prepares a window for an application-drawn title bar. macOS keeps its
    /// traffic-light buttons over a transparent, untitled, full-size title bar;
    /// leave about 78 logical units clear at the leading edge and 28 at the top.
    /// Windows and Linux remove the platform decorations, so the application also
    /// draws minimize/maximize/close buttons. Mark the bar with
    /// `Element::window_drag_region` so it moves the window.
    #[must_use]
    pub fn custom_title_bar(self) -> Self {
        if cfg!(target_os = "macos") {
            self.titlebar_transparent(true)
                .fullsize_content_view(true)
                .title_hidden(true)
        } else {
            self.decorations(false)
        }
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
        let invalid_position = matches!(self.attributes.position, Some(astrelis_winit::winit::dpi::Position::Logical(p)) if !p.x.is_finite() || !p.y.is_finite());
        let logical = |size: Option<Size>| match size {
            Some(Size::Logical(size)) => Some([size.width, size.height]),
            _ => None,
        };
        let (min, max) = (
            logical(self.attributes.min_inner_size),
            logical(self.attributes.max_inner_size),
        );
        let invalid_limits = min
            .iter()
            .chain(max.iter())
            .flatten()
            .any(|v| !v.is_finite() || *v <= 0.)
            || matches!((min, max), (Some(min), Some(max)) if min[0] > max[0] || min[1] > max[1]);
        if invalid_size
            || invalid_position
            || invalid_limits
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
    geometry: Cell<Option<WindowGeometry>>,
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
    /// Latest persistable normal bounds and maximized state. Before creation this
    /// is the requested size/placement when known; afterward it follows native
    /// observations and remains readable after close. It is a snapshot, not a
    /// reactive subscription. Position may be None on unsupported platforms.
    pub fn geometry(&self) -> Option<WindowGeometry> {
        self.life.geometry.get()
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
    fn is_animating(&self) -> bool;
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
    fn key(
        &mut self,
        runtime: &mut Runtime,
        event: crate::KeyEvent,
    ) -> Result<crate::InputResult, UiError>;
    fn command_info(&self, id: CommandId) -> Option<CommandInfo>;
    fn dispatch_command(
        &self,
        runtime: &mut Runtime,
        id: CommandId,
    ) -> Result<CommandStatus, UiError>;
    #[cfg(all(
        feature = "native-menus",
        any(target_os = "macos", target_os = "windows")
    ))]
    fn shortcut_command(&self, shortcut: &crate::Shortcut) -> Option<CommandId>;
    fn command_text_input(
        &mut self,
        runtime: &mut Runtime,
        event: TextInputEvent,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError>;
    fn hit_test(&self, point: [f32; 2]) -> Option<ElementId>;
    fn window_drag_region_at(&self, point: [f32; 2]) -> bool;
    fn set_focus_ring_policy(&mut self, policy: FocusRingPolicy);
    fn cursor_icon(&self) -> crate::Cursor;
    fn prepare_input(
        &mut self,
        runtime: &mut Runtime,
        metrics: WindowMetrics,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError>;
    fn pointer(
        &mut self,
        runtime: &mut Runtime,
        event: PointerEvent,
        painter: &mut UiPainter,
        extend: bool,
        click_count: u8,
    ) -> Result<bool, UiError>;
    fn text_input(
        &mut self,
        runtime: &mut Runtime,
        event: TextInputEvent,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError>;
    fn has_text_focus(&self) -> bool;
    fn has_text_client_focus(&self) -> bool;
    fn accepts_ime(&self) -> bool;
    fn focused_element(&self) -> Option<ElementId>;
    fn ime_reset_revision(&self) -> u64;
    fn selected_text(&self) -> Option<String>;
    fn ime_area(&mut self, painter: &mut UiPainter) -> Result<Option<Bounds>, UiError>;
    fn active(&mut self, active: bool) -> bool;
    fn caret_visible(&mut self, visible: bool);

    fn wheel(
        &mut self,
        runtime: &mut Runtime,
        point: [f32; 2],
        delta: [f32; 2],
        modifiers: crate::Modifiers,
    ) -> Result<bool, UiError>;
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
    fn is_animating(&self) -> bool {
        self.is_animating()
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
    fn key(
        &mut self,
        runtime: &mut Runtime,
        event: crate::KeyEvent,
    ) -> Result<crate::InputResult, UiError> {
        self.key(runtime, event)
    }
    fn command_info(&self, id: CommandId) -> Option<CommandInfo> {
        self.native_command_info(id)
    }
    fn dispatch_command(
        &self,
        runtime: &mut Runtime,
        id: CommandId,
    ) -> Result<CommandStatus, UiError> {
        self.dispatch_command_id(runtime, id)
    }
    #[cfg(all(
        feature = "native-menus",
        any(target_os = "macos", target_os = "windows")
    ))]
    fn shortcut_command(&self, shortcut: &crate::Shortcut) -> Option<CommandId> {
        self.shortcut_command(shortcut)
    }
    fn command_text_input(
        &mut self,
        runtime: &mut Runtime,
        event: TextInputEvent,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError> {
        self.native_text_input(runtime, event, painter)
    }
    fn hit_test(&self, point: [f32; 2]) -> Option<ElementId> {
        self.hit_test(point)
    }
    fn window_drag_region_at(&self, point: [f32; 2]) -> bool {
        self.window_drag_region_at(point)
    }
    fn set_focus_ring_policy(&mut self, policy: FocusRingPolicy) {
        self.set_focus_ring_policy(policy);
    }
    fn cursor_icon(&self) -> crate::Cursor {
        self.cursor()
    }
    fn prepare_input(
        &mut self,
        runtime: &mut Runtime,
        metrics: WindowMetrics,
        painter: &mut UiPainter,
    ) -> Result<bool, UiError> {
        if self.needs_prepare(runtime)? || self.measurement_generation() != painter.generation() {
            let size = metrics.logical_size();
            self.prepare(runtime, [size.width as f32, size.height as f32], painter)?;
            return Ok(true);
        }
        Ok(false)
    }
    fn pointer(
        &mut self,
        runtime: &mut Runtime,
        event: PointerEvent,
        painter: &mut UiPainter,
        extend: bool,
        click_count: u8,
    ) -> Result<bool, UiError> {
        self.pointer_with_text_clicks(runtime, event, painter, extend, click_count)
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
    fn has_text_client_focus(&self) -> bool {
        self.focused_text_client().is_some()
    }
    fn accepts_ime(&self) -> bool {
        self.accepts_ime()
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

    fn wheel(
        &mut self,
        runtime: &mut Runtime,
        point: [f32; 2],
        delta: [f32; 2],
        modifiers: crate::Modifiers,
    ) -> Result<bool, UiError> {
        Ok(self.wheel(runtime, point, delta, modifiers)?.changed)
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
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    Dialog(u64),
    Open {
        life: Rc<Life>,
        options: Box<WindowOptions>,
        factory: Box<Factory>,
    },
    Close(WindowId),
    RequestClose(WindowId),
    RequestQuit,
    Redraw(WindowId),
    Theme(WindowId),
    /// Validated font data or a family mapping for the shared painter.
    Font(FontChange),
    Exit,
}
/// A font registration queued from application code.
enum FontChange {
    Load(Arc<[u8]>),
    Map(crate::FontFamily, Arc<str>),
}
pub(crate) struct Commands {
    clipboard: RefCell<Option<arboard::Clipboard>>,
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    dialogs: RefCell<dialogs::State>,
    runtime: u64,
    next: Cell<u64>,
    queue: RefCell<VecDeque<Command>>,
    lives: RefCell<HashMap<WindowId, Rc<Life>>>,
    exited: Cell<bool>,
    mounts: RefCell<HashMap<crate::MountId, WindowId>>,
    theme: RefCell<Theme>,
    /// Some while the application theme follows a mode rather than an explicit theme.
    theme_mode: Cell<Option<ThemeMode>>,
    themes: RefCell<ThemeSet>,
    preferences: RefCell<SystemPreferences>,
}
fn record_mounts(commands: &Commands, window: WindowId, ui: &dyn HostedUi) {
    let ids: HashSet<_> = ui.mounts().into_iter().collect();
    let mut mounts = commands.mounts.borrow_mut();
    mounts.retain(|mount, owner| *owner != window || ids.contains(mount));
    for mount in ids {
        mounts.insert(mount, window);
    }
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
            clipboard: RefCell::new(None),
            #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
            dialogs: RefCell::new(dialogs::State::default()),
            next: Cell::new(1),
            queue: RefCell::new(VecDeque::new()),
            lives: RefCell::new(HashMap::new()),
            exited: Cell::new(false),
            mounts: RefCell::new(HashMap::new()),
            theme: RefCell::new(Theme::default()),
            theme_mode: Cell::new(None),
            themes: RefCell::new(ThemeSet::default()),
            preferences: RefCell::new(SystemPreferences::default()),
        }
    }
    /// Installs the application theme and queues its inheriting windows. `force`
    /// queues them even for an unchanged theme, so a mode change also updates
    /// whether their native title bars follow the system.
    fn install_theme(&self, theme: Theme, force: bool) {
        if *self.theme.borrow() == theme && !force {
            return;
        }
        *self.theme.borrow_mut() = theme.clone();
        for life in self.lives.borrow().values() {
            if life.inherits_theme.get() && life.alive.get() && !life.closing.get() {
                *life.theme.borrow_mut() = theme.clone();
                self.queue.borrow_mut().push_back(Command::Theme(life.id));
            }
        }
    }
    fn resolved_theme(&self) -> Option<Theme> {
        let mode = self.theme_mode.get()?;
        Some(
            self.themes
                .borrow()
                .resolve(mode, &self.preferences.borrow()),
        )
    }
    /// Stores a preference snapshot and re-resolves a mode theme; false if unchanged.
    fn set_preferences(&self, preferences: SystemPreferences) -> bool {
        if *self.preferences.borrow() == preferences {
            return false;
        }
        *self.preferences.borrow_mut() = preferences;
        if let Some(theme) = self.resolved_theme() {
            self.install_theme(theme, false);
        }
        true
    }
    /// Native title-bar appearance: follow the system while the window inherits a
    /// System-mode theme, otherwise match the window theme's scheme.
    fn native_theme(&self, life: &Life) -> Option<NativeTheme> {
        let follows = life.inherits_theme.get() && self.theme_mode.get() == Some(ThemeMode::System);
        (!follows).then(|| preferences::native_theme(ColorScheme::of(&life.theme.borrow())))
    }
    fn read_clipboard(&self) -> Result<String, arboard::Error> {
        clipboard(&mut self.clipboard.borrow_mut()).and_then(|c| c.get_text())
    }
    fn write_clipboard(&self, text: String) -> Result<(), arboard::Error> {
        clipboard(&mut self.clipboard.borrow_mut()).and_then(|c| c.set_text(text))
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
            geometry: Cell::new(geometry::Request::new(&options.attributes).pending()),
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
    /// Explicit window/subtree themes and literal styling remain unchanged. An
    /// explicit theme stops following the system until `set_theme_mode`.
    pub fn set_theme(&mut self, theme: Theme) -> Result<(), ApplicationError> {
        theme.validate()?;
        let commands = self.native_commands()?;
        let mode_changed = commands.theme_mode.replace(None).is_some();
        commands.install_theme(theme, mode_changed);
        Ok(())
    }
    /// Current theme mode, or None while an explicit application theme is in use.
    pub fn theme_mode(&self) -> Result<Option<ThemeMode>, ApplicationError> {
        Ok(self.native_commands()?.theme_mode.get())
    }
    /// Chooses the application theme by mode from the application's [`ThemeSet`]:
    /// System follows the OS appearance, high contrast and accent live; Light and
    /// Dark fix the scheme. Inheriting windows update like `set_theme`.
    pub fn set_theme_mode(&mut self, mode: ThemeMode) -> Result<(), ApplicationError> {
        let commands = self.native_commands()?;
        let changed = commands.theme_mode.replace(Some(mode)) != Some(mode);
        let theme = commands.resolved_theme().expect("theme mode was just set");
        commands.install_theme(theme, changed);
        Ok(())
    }
    /// Replaces the themes that modes choose between, for example to switch every
    /// mode to compact density. Applies immediately while a mode is active.
    pub fn set_theme_set(&mut self, themes: ThemeSet) -> Result<(), ApplicationError> {
        for theme in [&themes.light, &themes.dark, &themes.high_contrast] {
            theme.validate()?;
        }
        let commands = self.native_commands()?;
        *commands.themes.borrow_mut() = themes;
        if let Some(theme) = commands.resolved_theme() {
            commands.install_theme(theme, false);
        }
        Ok(())
    }
    /// Latest detected OS appearance and accessibility preferences. The host
    /// refreshes it on system appearance changes and window activation; see
    /// `Application::system_preferences_changed` to react to changes.
    pub fn system_preferences(&self) -> Result<SystemPreferences, ApplicationError> {
        Ok(self.native_commands()?.preferences.borrow().clone())
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
    /// Registers TrueType/OpenType data (including collections) with the
    /// application's shared painter and returns the faces it adds. The data is
    /// validated now and loaded before the next frame, after which every window
    /// re-measures its text. Use the returned family names with
    /// `FontFamily::Named`, or [`Self::map_font_family`] to make a family the
    /// default. Invalid data returns `ApplicationError::Ui(UiError::Text)`.
    /// Registration is a startup or settings-time operation, not a per-frame one.
    pub fn register_font(
        &mut self,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<Vec<crate::FontFace>, ApplicationError> {
        let commands = self.native_commands()?;
        let bytes = bytes.into();
        let faces = astrelis::TextSystem::new()
            .load_font_shared(bytes.clone())
            .map_err(UiError::from)?;
        commands
            .queue
            .borrow_mut()
            .push_back(Command::Font(FontChange::Load(bytes)));
        Ok(faces.into_iter().map(crate::painting::font_face).collect())
    }
    /// Resolves `family` to the loaded family `target` in every window, as
    /// [`UiPainter::map_family`] does; for example, map `FontFamily::SansSerif` to
    /// change the default face. Applied before the next frame.
    pub fn map_font_family(
        &mut self,
        family: impl Into<crate::FontFamily>,
        target: impl Into<Arc<str>>,
    ) -> Result<(), ApplicationError> {
        let commands = self.native_commands()?;
        commands
            .queue
            .borrow_mut()
            .push_back(Command::Font(FontChange::Map(family.into(), target.into())));
        Ok(())
    }
    /// Reads system clipboard text using the host's persistent clipboard owner.
    /// Empty/non-text content and platform failures are returned as native errors.
    pub fn read_clipboard_text(&self) -> Result<String, ApplicationError> {
        let commands = self.native_commands()?;
        let result = commands.read_clipboard();
        result.map_err(|e| ApplicationError::Native(Box::new(e)))
    }
    /// Replaces system clipboard text. The host retains clipboard ownership until
    /// shutdown, including on platforms that require the owner to remain alive.
    pub fn write_clipboard_text(
        &mut self,
        text: impl Into<String>,
    ) -> Result<(), ApplicationError> {
        let commands = self.native_commands()?;
        let result = commands.write_clipboard(text.into());
        result.map_err(|e| ApplicationError::Native(Box::new(e)))
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
    /// Requests a window close through Application::close_requested. Unlike
    /// close_window, a request does not mark the handle closing until accepted.
    pub fn request_close(&mut self, window: &WindowHandle) -> Result<(), ApplicationError> {
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
            .push_back(Command::RequestClose(window.id()));
        Ok(())
    }
    /// Requests shutdown through Application::quit_requested. A veto leaves the
    /// application live; invoke exit after asynchronous save/shutdown work completes.
    pub fn request_quit(&mut self) -> Result<(), ApplicationError> {
        self.native_commands()?
            .queue
            .borrow_mut()
            .push_back(Command::RequestQuit);
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
type GeometryHook = dyn FnMut(&WindowHandle, WindowGeometry, &mut AppContext<'_>);
type FileDropHook = dyn FnMut(&WindowHandle, &FileDropEvent, &mut AppContext<'_>);
type PreferencesHook = dyn FnMut(&SystemPreferences, &mut AppContext<'_>);
type CreatedHook = dyn FnMut(&WindowHandle, &mut AppContext<'_>);
type QuitHook = dyn FnMut(&mut AppContext<'_>) -> astrelis_winit::CloseResponse;
type CloseHook = dyn FnMut(&WindowHandle, &mut AppContext<'_>) -> astrelis_winit::CloseResponse;
type ExitHook = dyn FnOnce(&mut AppContext<'_>);
/// Desktop host over astrelis-winit. It owns native wiring and on-demand scheduling;
/// Runtime/Ui/UiPainter remain usable in custom hosts. Initialization runs once,
/// model/task progression continues independently of surface availability.
pub struct Application {
    #[cfg(feature = "native-menus")]
    menu_bar: Option<crate::NativeMenuBar>,
    theme: Theme,
    theme_mode: Option<ThemeMode>,
    themes: ThemeSet,
    preferences_changed: Option<Box<PreferencesHook>>,
    focus_ring: FocusRingPolicy,
    system_ui_font: bool,
    fonts: Vec<Arc<[u8]>>,
    families: Vec<(crate::FontFamily, Arc<str>)>,
    system_fonts: bool,
    accessibility: bool,
    executor: Option<Arc<dyn TaskExecutor>>,
    graphics: Option<GraphicsContext>,
    runner_options: RunnerOptions,
    prepare_graphics: Option<Box<GraphicsPrepareHook>>,
    render_graphics: Option<Box<GraphicsRenderHook>>,
    created: Option<Box<CreatedHook>>,
    geometry_changed: Option<Box<GeometryHook>>,
    file_drop: Option<Box<FileDropHook>>,
    close: Option<Box<CloseHook>>,
    quit: Option<Box<QuitHook>>,
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
            theme_mode: Some(ThemeMode::System),
            themes: ThemeSet::default(),
            preferences_changed: None,
            focus_ring: FocusRingPolicy::default(),
            system_ui_font: true,
            fonts: Vec::new(),
            families: Vec::new(),
            system_fonts: true,
            accessibility: true,
            executor: None,
            graphics: None,
            runner_options: RunnerOptions::default(),
            prepare_graphics: None,
            render_graphics: None,
            created: None,
            geometry_changed: None,
            file_drop: None,
            close: None,
            quit: None,
            #[cfg(feature = "native-menus")]
            menu_bar: None,
            exiting: None,
        }
    }
    /// Explicit default theme for windows without a WindowOptions theme. It
    /// replaces the default [`ThemeMode::System`], so the system appearance no
    /// longer changes it; use `theme_mode`/`themes` to keep following the system.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self.theme_mode = None;
        self
    }
    /// Chooses the default theme by mode (System by default): System follows the
    /// OS light/dark appearance, high contrast and accent color live.
    #[must_use]
    pub fn theme_mode(mut self, mode: ThemeMode) -> Self {
        self.theme_mode = Some(mode);
        self
    }
    /// Themes that modes choose between; the built-in presets by default.
    #[must_use]
    pub fn themes(mut self, themes: ThemeSet) -> Self {
        self.themes = themes;
        self
    }
    /// Receives each changed OS preference snapshot, after mode themes update, in
    /// a fresh application update with no source window. Read the snapshot at any
    /// time with `AppContext::system_preferences`, for example in initialization.
    #[must_use]
    pub fn system_preferences_changed(
        mut self,
        hook: impl FnMut(&SystemPreferences, &mut AppContext<'_>) + 'static,
    ) -> Self {
        self.preferences_changed = Some(Box::new(hook));
        self
    }
    /// When managed windows draw focus rings; keyboard-only by default.
    #[must_use]
    pub fn focus_ring(mut self, policy: FocusRingPolicy) -> Self {
        self.focus_ring = policy;
        self
    }
    /// Uses the platform's interface font for the generic sans-serif family when
    /// system font discovery finds one (default true); see
    /// [`crate::system_ui_font_families`]. Named families are unaffected.
    #[must_use]
    pub fn system_ui_font(mut self, enabled: bool) -> Self {
        self.system_ui_font = enabled;
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
    /// Receives the initial native geometry and changes to persistable normal
    /// bounds/maximized state. Fullscreen/minimized sizes are excluded. The hook
    /// runs in a fresh application update with no implicit source window. Retain
    /// the latest value in memory and debounce/persist off the event thread.
    #[must_use]
    pub fn window_geometry_changed(
        mut self,
        hook: impl FnMut(&WindowHandle, WindowGeometry, &mut AppContext<'_>) + 'static,
    ) -> Self {
        self.geometry_changed = Some(Box::new(hook));
        self
    }
    /// Receives native window-level file hover/cancel/drop notifications outside
    /// entity leases. No implicit event source or element hit target is invented;
    /// use the supplied managed window and update its model explicitly. Closing
    /// windows suppress delivery. File I/O belongs on background tasks. Support
    /// follows winit's backend; Wayland currently provides no file-drop events.
    #[must_use]
    pub fn file_drop(
        mut self,
        hook: impl FnMut(&WindowHandle, &FileDropEvent, &mut AppContext<'_>) + 'static,
    ) -> Self {
        self.file_drop = Some(Box::new(hook));
        self
    }
    /// Adds application font data, selecting it instead of default system discovery.
    /// Invalid data fails `run` when the first window is created. To validate data
    /// or learn its family names at runtime, use `AppContext::register_font`.
    #[must_use]
    pub fn font(mut self, bytes: impl Into<Arc<[u8]>>) -> Self {
        self.fonts.push(bytes.into());
        self.system_fonts = false;
        self
    }
    /// Resolves `family` to the loaded family `target` before shaping, for example
    /// `.map_font_family(FontFamily::SansSerif, "Inter")` to make a bundled face the
    /// default. See `UiPainter::map_family`.
    #[must_use]
    pub fn map_font_family(
        mut self,
        family: impl Into<crate::FontFamily>,
        target: impl Into<Arc<str>>,
    ) -> Self {
        self.families.push((family.into(), target.into()));
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
    /// Vetoes or defers requested Quit actions, for example while saving all
    /// documents. Explicit cx.exit is an already-decided exit and bypasses this hook.
    /// The default accepts Quit. Accepted Quit does not issue individual close hooks.
    #[must_use]
    pub fn quit_requested(
        mut self,
        hook: impl FnMut(&mut AppContext<'_>) -> astrelis_winit::CloseResponse + 'static,
    ) -> Self {
        self.quit = Some(Box::new(hook));
        self
    }
    /// Installs a native menu bar backed by current typed command resolution.
    /// macOS uses one application menu; Windows uses a menu per window. Linux/BSD
    /// attachment is unsupported by the winit host and returns an error at run.
    /// RXUI owns Muda's process-wide event handler while the application runs.
    #[cfg(feature = "native-menus")]
    #[must_use]
    pub fn menu_bar(mut self, menu: crate::NativeMenuBar) -> Self {
        self.menu_bar = Some(menu);
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
        for theme in [
            &self.themes.light,
            &self.themes.dark,
            &self.themes.high_contrast,
        ] {
            theme.validate()?;
        }
        #[cfg(all(
            feature = "native-menus",
            not(any(target_os = "macos", target_os = "windows"))
        ))]
        if self.menu_bar.is_some() {
            return Err(ApplicationError::Native(Box::new(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "RXUI native menu bars require macOS or Windows; Muda's Linux/BSD backend needs a GTK window",
            ))));
        }
        #[cfg(all(
            feature = "native-menus",
            any(target_os = "macos", target_os = "windows")
        ))]
        let menus = self.menu_bar.map(menu_backend::Menus::new);
        let mut builder = astrelis_winit::winit::event_loop::EventLoop::<Wake>::with_user_event();
        #[cfg(all(feature = "native-menus", target_os = "windows"))]
        if let Some(menus) = &menus {
            menus.hook(&mut builder);
        }
        let mut runner = Runner::from_event_loop(
            builder
                .build()
                .map_err(|e| ApplicationError::Native(Box::new(e)))?,
        )
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
        *commands.themes.borrow_mut() = self.themes;
        commands.theme_mode.set(self.theme_mode);
        if let Some(theme) = commands.resolved_theme() {
            *commands.theme.borrow_mut() = theme;
        }
        *runtime.inner.native.borrow_mut() = Some(commands.clone());
        let mut host = Host {
            runtime,
            commands,
            initialize: Some(initialize),
            windows: HashMap::new(),
            painter: None,
            fonts: self.fonts,
            families: self.families,
            system_fonts: self.system_fonts,
            active: false,
            prepare_graphics: self.prepare_graphics,
            render_graphics: self.render_graphics,
            created: self.created,
            geometry_changed: self.geometry_changed,
            file_drop: self.file_drop,
            preferences_changed: self.preferences_changed,
            preferences_started: false,
            focus_ring: self.focus_ring,
            system_ui_font: self.system_ui_font,
            close: self.close,
            quit: self.quit,
            focused_window: None,
            #[cfg(all(
                feature = "native-menus",
                any(target_os = "macos", target_os = "windows")
            ))]
            menus,
            exiting: self.exiting,
            accessibility_enabled: self.accessibility,
            started: Instant::now(),
        };
        runner
            .run(&mut host)
            .map_err(|e| ApplicationError::Native(Box::new(e)))
    }
}
enum Wake {
    Geometry(NativeWindowId),
    /// Pushed preference snapshot (the Linux/BSD portal watcher).
    Preferences(SystemPreferences),
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    Dialog(dialogs::Packet),
    #[cfg(all(
        feature = "native-menus",
        any(target_os = "macos", target_os = "windows")
    ))]
    Menu(muda::MenuEvent),
    Tasks,
    Accessibility(accesskit_winit::Event),
}
impl From<accesskit_winit::Event> for Wake {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}
#[derive(Default)]
struct ClickTracker {
    last: Option<(Instant, [f32; 2], Option<ElementId>, u8)>,
}
impl ClickTracker {
    fn motion(&mut self, point: [f32; 2]) {
        if self
            .last
            .is_some_and(|(_, p, _, _)| (p[0] - point[0]).hypot(p[1] - point[1]) > 4.)
        {
            self.last = None;
        }
    }
    fn press(&mut self, now: Instant, point: [f32; 2], target: Option<ElementId>) -> u8 {
        let count = self
            .last
            .filter(|(at, p, id, count)| {
                now.saturating_duration_since(*at) <= Duration::from_millis(500)
                    && (p[0] - point[0]).hypot(p[1] - point[1]) <= 4.
                    && *id == target
                    && *count < 3
            })
            .map_or(1, |(_, _, _, count)| count + 1);
        self.last = Some((now, point, target, count));
        count
    }
}
struct HostedWindow {
    life: Rc<Life>,
    ui: Option<Box<dyn HostedUi>>,
    factory: Option<Box<Factory>>,
    geometry_request: geometry::Request,
    geometry_pending: bool,
    restore_geometry: bool,
    background: Option<Color>,
    input: WindowInput,
    blink_at: Instant,
    caret_visible: bool,
    accessibility: Option<accesskit_winit::Adapter>,
    accesskit: AccessKitTree,
    accessibility_active: bool,
    /// The default clear is transparent.
    transparent: bool,
    /// False when native attributes chose an explicit native theme.
    sync_native_theme: bool,
    /// Native appearance last applied, None before the first sync.
    native_theme: Option<Option<NativeTheme>>,
}
/// Matches the native title bar to the window theme; see `Commands::native_theme`.
fn sync_native_theme(commands: &Commands, window: &mut HostedWindow) {
    if !window.sync_native_theme {
        return;
    }
    let wanted = commands.native_theme(&window.life);
    if window.native_theme != Some(wanted)
        && let Some(native) = window.life.native.borrow().as_ref()
    {
        native.set_theme(wanted);
        window.native_theme = Some(wanted);
    }
}
/// Stores a new snapshot; a changed one updates mode themes, then notifies.
fn deliver_preferences(
    runtime: &mut Runtime,
    commands: &Commands,
    preferences: SystemPreferences,
    hook: &mut Option<Box<PreferencesHook>>,
) -> bool {
    let reduce_motion = commands.preferences.borrow().reduce_motion;
    if commands.exited.get() || !commands.set_preferences(preferences.clone()) {
        return false;
    }
    // Only a changed OS setting replaces the runtime's, so an application override
    // survives unrelated preference changes; the hook below can still override.
    if preferences.reduce_motion != reduce_motion {
        runtime.set_reduce_motion(preferences.reduce_motion);
    }
    if let Some(hook) = hook {
        runtime.update(|cx| hook(&preferences, cx));
    }
    true
}
struct Host<F> {
    focused_window: Option<NativeWindowId>,
    #[cfg(all(
        feature = "native-menus",
        any(target_os = "macos", target_os = "windows")
    ))]
    menus: Option<menu_backend::Menus>,
    runtime: Runtime,
    commands: Rc<Commands>,
    initialize: Option<F>,
    windows: HashMap<NativeWindowId, HostedWindow>,
    painter: Option<UiPainter>,
    fonts: Vec<Arc<[u8]>>,
    families: Vec<(crate::FontFamily, Arc<str>)>,
    system_fonts: bool,
    active: bool,
    prepare_graphics: Option<Box<GraphicsPrepareHook>>,
    render_graphics: Option<Box<GraphicsRenderHook>>,
    created: Option<Box<CreatedHook>>,
    geometry_changed: Option<Box<GeometryHook>>,
    file_drop: Option<Box<FileDropHook>>,
    preferences_changed: Option<Box<PreferencesHook>>,
    /// Initial preferences were read and any watcher was started.
    preferences_started: bool,
    focus_ring: FocusRingPolicy,
    system_ui_font: bool,
    close: Option<Box<CloseHook>>,
    quit: Option<Box<QuitHook>>,
    exiting: Option<Box<ExitHook>>,
    accessibility_enabled: bool,
    /// Origin of animation frame times.
    started: Instant,
}
impl<F> Drop for Host<F> {
    fn drop(&mut self) {
        // Platform adapters release hooks while their native windows still exist,
        // including run failures that bypass the normal exiting callback.
        #[cfg(all(
            feature = "native-menus",
            any(target_os = "macos", target_os = "windows")
        ))]
        drop(self.menus.take());
        for (_, mut window) in self.windows.drain() {
            drop(window.accessibility.take());
            window.life.alive.set(false);
            window.life.native.borrow_mut().take();
        }
    }
}
impl<F> Host<F> {
    /// Re-reads preferences after an appearance change or activation. `hint` is
    /// winit's reported appearance for the event's window.
    fn refresh_preferences(&mut self, cx: &NativeContext<'_, Wake>, hint: Option<NativeTheme>) {
        let system = cx.event_loop().system_theme().or(hint);
        let preferences = if preferences::REFRESH_ON_FOCUS {
            preferences::detect(system)
        } else {
            // The portal watcher owns the other values on Linux/BSD.
            let current = self.commands.preferences.borrow().clone();
            SystemPreferences {
                color_scheme: system.map(preferences::scheme).or(current.color_scheme),
                ..current
            }
        };
        deliver_preferences(
            &mut self.runtime,
            &self.commands,
            preferences,
            &mut self.preferences_changed,
        );
    }
    fn invoke_command(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        window: Option<NativeWindowId>,
        command: CommandId,
    ) -> Result<CommandStatus, ApplicationError> {
        if let Some(id) = window {
            let Some(native) = cx.window(id) else {
                return Ok(CommandStatus::Unhandled);
            };
            let Some(window) = self.windows.get_mut(&id).filter(|w| !w.life.closing.get()) else {
                return Ok(CommandStatus::Unhandled);
            };
            let (Some(ui), Some(painter)) = (&mut window.ui, &mut self.painter) else {
                return Ok(CommandStatus::Unhandled);
            };
            if ui.prepare_input(&mut self.runtime, native.metrics(), painter)? {
                record_mounts(&self.commands, window.life.id, ui.as_ref());
            }
            let status = ui.dispatch_command(&mut self.runtime, command)?;
            if status != CommandStatus::Unhandled {
                return Ok(status);
            }
            let Some(info) = ui.command_info(command) else {
                return Ok(CommandStatus::Unhandled);
            };
            if !info.enabled {
                return Ok(CommandStatus::Disabled);
            }
            use crate::standard_commands::*;
            let mut changed = false;
            if command == CommandId::of::<Copy>() || command == CommandId::of::<Cut>() {
                if let Some(text) = ui.selected_text() {
                    match self.commands.write_clipboard(text) {
                        Ok(()) if command == CommandId::of::<Cut>() => {
                            changed = ui.command_text_input(
                                &mut self.runtime,
                                TextInputEvent::Insert(String::new()),
                                painter,
                            )?
                        }
                        Err(error) => eprintln!("RXUI clipboard: {error}"),
                        _ => {}
                    }
                }
            } else if command == CommandId::of::<Paste>() {
                match self.commands.read_clipboard() {
                    Ok(text) => {
                        changed = ui.command_text_input(
                            &mut self.runtime,
                            TextInputEvent::Paste(text),
                            painter,
                        )?
                    }
                    Err(error) => eprintln!("RXUI clipboard: {error}"),
                }
            } else if command == CommandId::of::<Undo>() || command == CommandId::of::<Redo>() {
                changed = ui.command_text_input(
                    &mut self.runtime,
                    if command == CommandId::of::<Undo>() {
                        TextInputEvent::Undo
                    } else {
                        TextInputEvent::Redo
                    },
                    painter,
                )?;
            } else if command == CommandId::of::<SelectAll>() {
                changed =
                    ui.command_text_input(&mut self.runtime, TextInputEvent::SelectAll, painter)?;
            } else if command == CommandId::of::<CloseWindow>() {
                self.commands
                    .queue
                    .borrow_mut()
                    .push_back(Command::RequestClose(window.life.id));
            } else if command == CommandId::of::<Quit>() {
                self.commands
                    .queue
                    .borrow_mut()
                    .push_back(Command::RequestQuit);
            } else {
                return Ok(CommandStatus::Unhandled);
            }
            if changed {
                window.caret_visible = true;
                window.blink_at = Instant::now() + Duration::from_millis(500);
                ui.caret_visible(true);
                cx.request_redraw(id)
                    .map_err(|e| ApplicationError::Native(Box::new(e)))?;
            }
            self.sync_text_platform(cx, id)?;
            Ok(CommandStatus::Handled)
        } else {
            let status = self
                .runtime
                .update(|cx| cx.dispatch_application_command(command))?;
            if status != CommandStatus::Unhandled {
                return Ok(status);
            }
            if command == CommandId::of::<crate::standard_commands::Quit>() {
                self.commands
                    .queue
                    .borrow_mut()
                    .push_back(Command::RequestQuit);
                return Ok(CommandStatus::Handled);
            }
            Ok(CommandStatus::Unhandled)
        }
    }
    #[cfg(all(
        feature = "native-menus",
        any(target_os = "macos", target_os = "windows")
    ))]
    fn sync_menus(&mut self, cx: &mut NativeContext<'_, Wake>) -> Result<(), ApplicationError> {
        let Some(menus) = &mut self.menus else {
            return Ok(());
        };
        #[cfg(all(feature = "native-dialogs", target_os = "macos"))]
        // Cocoa sheets may leave their parent's winit focus flag set. Route
        // editing to Cocoa while that parent's dialog is open; switching to a
        // different managed window restores its RXUI command handlers.
        menus.native_editing({
            let dialogs = self.commands.dialogs.borrow();
            dialogs.running()
                && self
                    .focused_window
                    .and_then(|id| self.windows.get(&id))
                    .is_none_or(|window| dialogs.busy(window.life.id))
        })?;
        #[cfg(target_os = "macos")]
        let ids = vec![self.focused_window];
        #[cfg(target_os = "windows")]
        let ids: Vec<_> = self.windows.keys().copied().map(Some).collect();
        for id in ids {
            let source = id.and_then(|id| self.windows.get(&id)).map(|w| w.life.id);
            // Reconcile only when necessary. No GPU preparation or polling is added.
            let ui = id
                .and_then(|id| self.windows.get_mut(&id).filter(|w| !w.life.closing.get()))
                .and_then(|w| w.ui.as_mut());
            if let (Some(id), Some(ui), Some(painter)) = (id, ui, self.painter.as_mut())
                && let Some(native) = cx.window(id)
                && ui.prepare_input(&mut self.runtime, native.metrics(), painter)?
                && let Some(source) = source
            {
                record_mounts(&self.commands, source, ui.as_ref());
            }
            let ui = id
                .and_then(|id| self.windows.get(&id).filter(|w| !w.life.closing.get()))
                .and_then(|w| w.ui.as_deref());
            menus.update(
                id,
                |command| {
                    if let Some(ui) = ui {
                        ui.command_info(command)
                    } else {
                        self.runtime
                            .update(|cx| cx.application_command(command).map(|a| a.info()))
                            .or_else(|| {
                                crate::commands::standard_info(
                                    command,
                                    false,
                                    crate::commands::StandardEditing::default(),
                                    false,
                                )
                            })
                    }
                },
                |s| ui.and_then(|ui| ui.shortcut_command(s)),
            )?;
        }
        Ok(())
    }
    fn sync_mounts(&self, id: NativeWindowId) {
        let window = &self.windows[&id];
        if let Some(ui) = &window.ui {
            record_mounts(&self.commands, window.life.id, ui.as_ref());
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
        if let Some(painter) = &mut self.painter {
            window.input.sync(ui.as_mut(), painter, native.window())?;
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
        if ui.prepare_input(&mut self.runtime, native.metrics(), painter)? {
            record_mounts(&self.commands, window.life.id, ui.as_ref());
        }
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
        profiling::scope!("rxui::native::progress");
        if cx.event_loop().exiting() {
            return Ok(());
        }
        if !self.commands.exited.get() {
            self.runtime.poll_tasks();
            self.runtime.flush()?;
        }
        #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
        self.commands
            .dialogs
            .borrow_mut()
            .prune(&self.runtime.inner, self.commands.exited.get());
        let mut pending = std::mem::take(&mut *self.commands.queue.borrow_mut());
        let mut deferred = VecDeque::new();
        let mut request_budget = 1024;
        while !pending.is_empty() {
            for command in pending {
                if matches!(command, Command::RequestClose(_) | Command::RequestQuit) {
                    if request_budget == 0 {
                        return Err(ApplicationError::Native(Box::new(std::io::Error::other(
                            "RXUI close/quit hooks repeatedly requested another close/quit",
                        ))));
                    }
                    request_budget -= 1;
                }
                match command {
                    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
                    Command::Dialog(id) => {
                        if !self.active || !self.start_dialog(cx, id)? {
                            deferred.push_back(Command::Dialog(id));
                        }
                    }
                    Command::Open {
                        life,
                        mut options,
                        factory,
                    } => {
                        if life.closing.get() || self.commands.exited.get() {
                            life.alive.set(false);
                            self.commands.lives.borrow_mut().remove(&life.id);
                            continue;
                        }
                        if !self.active {
                            deferred.push_back(Command::Open {
                                life,
                                options,
                                factory,
                            });
                            continue;
                        }
                        if options.restore_geometry {
                            options.attributes = geometry::restore_attributes(
                                options.attributes,
                                &geometry::monitors(cx.event_loop()),
                            );
                        }
                        let geometry_request = geometry::Request::new(&options.attributes);
                        life.geometry.set(geometry_request.pending());
                        // Position setters unmaximize a window. Restore its normal
                        // outer frame before applying the saved maximized state.
                        if options.restore_geometry {
                            options.attributes.maximized = false;
                        }
                        let transparent = options.attributes.transparent;
                        let sync_native_theme = options.attributes.preferred_theme.is_none();
                        let native = create_window(cx, options.attributes, options.surface)?;
                        self.windows.insert(
                            native,
                            HostedWindow {
                                life,
                                ui: None,
                                factory: Some(factory),
                                geometry_request,
                                geometry_pending: false,
                                restore_geometry: options.restore_geometry,
                                background: options.background,
                                input: WindowInput::default(),
                                blink_at: Instant::now() + Duration::from_millis(500),
                                caret_visible: true,
                                accessibility: None,
                                accesskit: AccessKitTree::new(),
                                accessibility_active: false,
                                transparent,
                                sync_native_theme,
                                native_theme: None,
                            },
                        );
                    }
                    Command::Close(id) => {
                        #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
                        if self.commands.dialogs.borrow().busy(id) {
                            deferred.push_back(Command::Close(id));
                            continue;
                        }
                        if let Some(native) = self
                            .windows
                            .iter()
                            .find(|(_, window)| window.life.id == id)
                            .map(|(id, _)| *id)
                        {
                            self.observe_geometry(native, false);
                            cx.close_window(native)
                                .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                        }
                    }
                    Command::RequestClose(id) => {
                        let life = self.commands.lives.borrow().get(&id).cloned();
                        if let Some(life) =
                            life.filter(|life| life.alive.get() && !life.closing.get())
                        {
                            let handle = WindowHandle { life };
                            let response = self
                                .close
                                .as_mut()
                                .map_or(astrelis_winit::CloseResponse::Close, |hook| {
                                    self.runtime.update(|cx| hook(&handle, cx))
                                });
                            if response == astrelis_winit::CloseResponse::Close {
                                handle.life.closing.set(true);
                                #[cfg(all(
                                    feature = "native-dialogs",
                                    not(target_arch = "wasm32")
                                ))]
                                if self.commands.dialogs.borrow().busy(id) {
                                    deferred.push_back(Command::Close(id));
                                    continue;
                                }
                                if let Some(native) = self
                                    .windows
                                    .iter()
                                    .find(|(_, w)| w.life.id == id)
                                    .map(|(id, _)| *id)
                                {
                                    cx.close_window(native)
                                        .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                                }
                            }
                        }
                    }
                    Command::RequestQuit => {
                        let response = self
                            .quit
                            .as_mut()
                            .map_or(astrelis_winit::CloseResponse::Close, |hook| {
                                self.runtime.update(hook)
                            });
                        if response == astrelis_winit::CloseResponse::Close {
                            self.commands.exited.set(true);
                            #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
                            if self.commands.dialogs.borrow().running() {
                                deferred.push_back(Command::Exit);
                                continue;
                            }
                            cx.exit();
                            break;
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
                            sync_native_theme(&self.commands, window);
                            if cx.window(*native).is_some() {
                                cx.request_redraw(*native)
                                    .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                            }
                        }
                    }
                    Command::Font(change) => {
                        // Before the shared painter exists, join the startup fonts.
                        match (&mut self.painter, change) {
                            (Some(painter), FontChange::Load(bytes)) => {
                                painter.register_font(bytes)?;
                            }
                            (Some(painter), FontChange::Map(family, target)) => {
                                painter.map_family(family, target)
                            }
                            (None, FontChange::Load(bytes)) => self.fonts.push(bytes),
                            (None, FontChange::Map(family, target)) => {
                                self.families.push((family, target))
                            }
                        }
                        // A new font generation re-measures each window as it prepares.
                        for native in self.windows.keys() {
                            if cx.window(*native).is_some() {
                                cx.request_redraw(*native)
                                    .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                            }
                        }
                    }
                    Command::Exit => {
                        #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
                        if self.commands.dialogs.borrow().running() {
                            deferred.push_back(Command::Exit);
                            continue;
                        }
                        cx.exit();
                        self.commands.exited.set(true);
                        break;
                    }
                }
            }
            if cx.event_loop().exiting() {
                break;
            }
            // A veto hook may open a confirmation window, request a redraw or decide
            // to close/exit immediately. Apply those operations before going idle.
            pending = std::mem::take(&mut *self.commands.queue.borrow_mut());
        }
        self.commands.queue.borrow_mut().extend(deferred);
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
        #[cfg(all(
            feature = "native-menus",
            any(target_os = "macos", target_os = "windows")
        ))]
        self.sync_menus(cx)?;
        Ok(())
    }
}

impl<F: FnOnce(&mut AppContext<'_>) -> Result<(), ApplicationError>> Handler for Host<F> {
    type Message = Wake;
    type Error = ApplicationError;
    fn resumed(&mut self, cx: &mut NativeContext<'_, Wake>) -> Result<(), Self::Error> {
        self.active = true;
        if !self.preferences_started {
            // Before initialization, so its windows open with the system theme.
            self.preferences_started = true;
            let detected = preferences::detect(cx.event_loop().system_theme());
            self.commands.set_preferences(detected);
            let proxy = cx.proxy();
            preferences::watch(move |p| proxy.send_event(Wake::Preferences(p)).is_ok());
        }
        #[cfg(all(
            feature = "native-menus",
            any(target_os = "macos", target_os = "windows")
        ))]
        if let Some(menus) = &mut self.menus {
            menus.start(cx.proxy())?;
        }
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
                painter.register_font(bytes.clone())?;
            }
            for (family, target) in &self.families {
                painter.map_family(family.clone(), target.clone());
            }
            if self.system_ui_font && self.system_fonts {
                let family = system_ui_family(painter.fonts().fonts().flat_map(|f| f.families));
                painter.set_sans_serif_family(family);
            }
            self.painter = Some(painter);
        }
        let window = self.windows.get_mut(&id).unwrap();
        let native = cx.window(id).unwrap().window();
        *window.life.native.borrow_mut() = Some(native.clone());
        let requested = window.geometry_request.resolve(native.scale_factor());
        if window.restore_geometry && native.fullscreen().is_none() {
            // macOS creation attributes position the content area. Correct the
            // persisted outer frame while hidden, then maximize if requested.
            if let Some(position) = requested.and_then(|g| g.outer_position) {
                native.set_outer_position(astrelis_winit::winit::dpi::PhysicalPosition::new(
                    position[0],
                    position[1],
                ));
            }
            if requested.is_some_and(|g| g.maximized) {
                native.set_maximized(true);
            }
        }
        window
            .life
            .geometry
            .set(geometry::initial(native, requested));
        window.ui = Some(window.factory.take().unwrap()(&mut self.runtime)?);
        let ui = window.ui.as_mut().unwrap();
        ui.set_theme(window.life.theme.borrow().clone())?;
        ui.set_focus_ring_policy(self.focus_ring);
        sync_native_theme(&self.commands, window);
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
        #[cfg(all(
            feature = "native-menus",
            any(target_os = "macos", target_os = "windows")
        ))]
        if let Some(menus) = &mut self.menus {
            menus.attach(id, cx.window(id).unwrap().window().clone())?;
        }
        self.observe_geometry(id, true);
        self.sync_mounts(id);
        self.progress(cx, None)
    }
    fn window_event(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        if self.commands.exited.get() {
            return self.progress(cx, None);
        }
        if self.windows.get(&id).is_none_or(|w| w.life.closing.get()) {
            return self.progress(cx, None);
        }
        match event {
            WindowEvent::ThemeChanged(theme) => self.refresh_preferences(cx, Some(theme)),
            // Accessibility settings change while another app is active.
            WindowEvent::Focused(true) if preferences::REFRESH_ON_FOCUS => {
                self.refresh_preferences(cx, None)
            }
            _ => {}
        }
        if matches!(
            event,
            WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Focused(_)
        ) {
            self.queue_geometry(cx, id)?;
        }
        if let Some(event) = FileDropEvent::from_window_event(&event) {
            let handle = WindowHandle {
                life: self.windows[&id].life.clone(),
            };
            deliver_file_drop(&mut self.runtime, &handle, &event, &mut self.file_drop);
            return self.progress(cx, None);
        }
        if let Some(window) = self.windows.get_mut(&id)
            && let Some(adapter) = &mut window.accessibility
            && let Some(native) = window.life.native.borrow().as_ref()
        {
            adapter.process_event(native, &event);
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            return Ok(());
        }
        let (mut changed, mut key_prevented) = {
            let Some(window) = self.windows.get_mut(&id) else {
                return Ok(());
            };
            let (Some(ui), Some(painter), Some(native)) =
                (&mut window.ui, &mut self.painter, cx.window(id))
            else {
                return Ok(());
            };
            if ui.prepare_input(&mut self.runtime, native.metrics(), painter)? {
                record_mounts(&self.commands, window.life.id, ui.as_ref());
            }
            window.input.key(&mut self.runtime, ui.as_mut(), &event)?
        };
        if !key_prevented && let Some((command, repeat)) = self.windows[&id].input.shortcut(&event)
        {
            // Standard lifecycle requests do not repeat while a save/veto is
            // outstanding. Editing still receives normal key repetition.
            let status = if repeat && WindowInput::lifecycle(command) {
                CommandStatus::Disabled
            } else {
                self.invoke_command(cx, Some(id), command)?
            };
            key_prevented = status != CommandStatus::Unhandled;
            changed |= status == CommandStatus::Handled;
        }
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        let (Some(ui), Some(painter), Some(native)) =
            (&mut window.ui, &mut self.painter, cx.window(id))
        else {
            return Ok(());
        };
        if matches!(event, WindowEvent::Focused(true)) {
            self.focused_window = Some(id);
        }
        changed |= window.input.event(
            &mut self.runtime,
            ui.as_mut(),
            painter,
            native.metrics().scale_factor(),
            &event,
            key_prevented,
        )?;
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
            Wake::Geometry(id) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.geometry_pending = false;
                }
                self.observe_geometry(id, false);
                self.progress(cx, None)
            }
            #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
            Wake::Dialog(packet) => {
                self.finish_dialog(packet);
                self.progress(cx, None)
            }
            #[cfg(all(
                feature = "native-menus",
                any(target_os = "macos", target_os = "windows")
            ))]
            Wake::Menu(event) => {
                if self.commands.exited.get() {
                    return self.progress(cx, None);
                }
                // Complete outstanding updates before resolving the identity again.
                self.progress(cx, None)?;
                if let Some((window, command)) = self
                    .menus
                    .as_ref()
                    .and_then(|m| m.resolve(&event, self.focused_window))
                {
                    self.invoke_command(cx, window, command)?;
                }
                self.progress(cx, None)
            }
            Wake::Tasks => self.progress(cx, None),
            Wake::Preferences(preferences) => {
                let preferences = SystemPreferences {
                    // Keep winit's appearance where the portal has none.
                    color_scheme: preferences.color_scheme.or(self
                        .commands
                        .preferences
                        .borrow()
                        .color_scheme),
                    ..preferences
                };
                deliver_preferences(
                    &mut self.runtime,
                    &self.commands,
                    preferences,
                    &mut self.preferences_changed,
                );
                self.progress(cx, None)
            }
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
                        if ui.prepare_input(&mut self.runtime, native.metrics(), painter)? {
                            record_mounts(&self.commands, window.life.id, ui.as_ref());
                        }
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
        self.observe_geometry(id, false);
        let handle = WindowHandle {
            life: self.windows[&id].life.clone(),
        };
        let response = if let Some(hook) = &mut self.close {
            self.runtime.update(|cx| hook(&handle, cx))
        } else {
            astrelis_winit::CloseResponse::Close
        };
        #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
        if response == astrelis_winit::CloseResponse::Close
            && self.commands.dialogs.borrow().busy(handle.id())
        {
            handle.life.closing.set(true);
            self.commands
                .queue
                .borrow_mut()
                .push_back(Command::Close(handle.id()));
            self.progress(cx, None)?;
            return Ok(astrelis_winit::CloseResponse::KeepOpen);
        }
        self.progress(cx, None)?;
        Ok(response)
    }
    fn prepare(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<PrepareAction, Self::Error> {
        // Views that requested an animation frame become dirty before this frame's
        // preparation; progress then also redraws their other windows.
        self.runtime.begin_frame(self.started.elapsed());
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
        if window.life.closing.get() || self.commands.exited.get() {
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
        if self.runtime.animation_frame_requested() {
            for (native, window) in &self.windows {
                if !window.life.closing.get()
                    && cx.window(*native).is_some()
                    && window.ui.as_ref().is_some_and(|ui| {
                        // Retained transitions redraw only their own window.
                        ui.is_animating()
                            || ui
                                .mounts()
                                .into_iter()
                                .any(|mount| self.runtime.inner.frame_requested(mount))
                    })
                {
                    cx.request_redraw(*native)
                        .map_err(|e| ApplicationError::Native(Box::new(e)))?;
                }
            }
        }
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
        let c = hosted.background.unwrap_or_else(|| {
            if hosted.transparent {
                [0.; 4]
            } else {
                hosted.life.theme.borrow().palette().background
            }
        });
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
        #[cfg(all(
            feature = "native-menus",
            any(target_os = "macos", target_os = "windows")
        ))]
        if let Some(menus) = &mut self.menus {
            menus.detach(id);
        }
        if self.focused_window == Some(id) {
            self.focused_window = None;
        }
        if let Some(mut window) = self.windows.remove(&id) {
            drop(window.accessibility.take());
            if let Some(ui) = &mut window.ui
                && let Some(painter) = &mut self.painter
            {
                ui.pointer(
                    &mut self.runtime,
                    PointerEvent::Cancelled,
                    painter,
                    false,
                    1,
                )?;
            }
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
        #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
        self.commands.dialogs.borrow_mut().abandon();
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

/// Creates a native window; a transparent one first tries non-opaque surface
/// alpha modes, falling back to the requested settings where none is offered.
fn create_window(
    cx: &mut NativeContext<'_, Wake>,
    attributes: WindowAttributes,
    surface: SurfaceSettings,
) -> Result<NativeWindowId, ApplicationError> {
    use wgpu::CompositeAlphaMode as Alpha;
    let automatic = surface
        .surface_options(astrelis_winit::winit::dpi::PhysicalSize::new(1, 1))
        .alpha_mode
        == Alpha::Auto;
    if attributes.transparent && automatic {
        for mode in [Alpha::PreMultiplied, Alpha::PostMultiplied] {
            match cx.create_window(attributes.clone(), surface.alpha_mode(mode)) {
                Ok(id) => return Ok(id),
                Err(astrelis_winit::WindowError::Graphics(
                    astrelis::Error::UnsupportedPresentation { .. },
                )) => continue,
                Err(e) => return Err(ApplicationError::Native(Box::new(e))),
            }
        }
    }
    cx.create_window(attributes, surface)
        .map_err(|e| ApplicationError::Native(Box::new(e)))
}
/// First platform interface family among the loaded font families.
fn system_ui_family(loaded: impl Iterator<Item = String>) -> Option<Arc<str>> {
    let loaded: HashSet<String> = loaded.collect();
    crate::system_ui_font_families()
        .iter()
        .find(|name| loaded.contains(**name))
        .map(|name| Arc::from(*name))
}
fn input_modifiers(m: ModifiersState) -> crate::Modifiers {
    crate::Modifiers {
        shift: m.shift_key(),
        control: m.control_key(),
        alt: m.alt_key(),
        meta: m.super_key(),
    }
}
fn input_button(b: MouseButton) -> crate::PointerButton {
    match b {
        MouseButton::Right => crate::PointerButton::Secondary,
        MouseButton::Middle => crate::PointerButton::Middle,
        _ => crate::PointerButton::Primary,
    }
}
fn keyboard_key(key: &Key) -> crate::KeyboardKey {
    use crate::KeyboardKey as K;
    match key {
        Key::Character(s) => K::Character(s.to_string()),
        Key::Named(NamedKey::Escape) => K::Escape,
        Key::Named(NamedKey::Enter) => K::Enter,
        Key::Named(NamedKey::Space) => K::Space,
        Key::Named(NamedKey::Tab) => K::Tab,
        Key::Named(NamedKey::ArrowLeft) => K::ArrowLeft,
        Key::Named(NamedKey::ArrowRight) => K::ArrowRight,
        Key::Named(NamedKey::ArrowUp) => K::ArrowUp,
        Key::Named(NamedKey::ArrowDown) => K::ArrowDown,
        Key::Named(NamedKey::Home) => K::Home,
        Key::Named(NamedKey::End) => K::End,
        Key::Named(NamedKey::PageUp) => K::PageUp,
        Key::Named(NamedKey::PageDown) => K::PageDown,
        Key::Named(NamedKey::Backspace) => K::Backspace,
        Key::Named(NamedKey::Delete) => K::Delete,
        other => K::Other(format!("{other:?}")),
    }
}
/// Winit icon for a portable cursor; `None` hides the pointer.
fn native_cursor(cursor: crate::Cursor) -> Option<astrelis_winit::winit::window::CursorIcon> {
    use crate::Cursor as R;
    use astrelis_winit::winit::window::CursorIcon as C;
    Some(match cursor {
        R::Default => C::Default,
        R::Pointer => C::Pointer,
        R::Text => C::Text,
        R::Grab => C::Grab,
        R::Grabbing => C::Grabbing,
        R::ResizeHorizontal | R::ResizeColumn => C::ColResize,
        R::ResizeVertical | R::ResizeRow => C::RowResize,
        R::Crosshair => C::Crosshair,
        R::NotAllowed => C::NotAllowed,
        R::ResizeNorthSouth => C::NsResize,
        R::ResizeEastWest => C::EwResize,
        R::ResizeNorthEastSouthWest => C::NeswResize,
        R::ResizeNorthWestSouthEast => C::NwseResize,
        R::Move => C::Move,
        R::Wait => C::Wait,
        R::Progress => C::Progress,
        R::Help => C::Help,
        R::ZoomIn => C::ZoomIn,
        R::ZoomOut => C::ZoomOut,
        R::ContextMenu => C::ContextMenu,
        R::Copy => C::Copy,
        R::Alias => C::Alias,
        R::NoDrop => C::NoDrop,
        R::VerticalText => C::VerticalText,
        R::Cell => C::Cell,
        R::Hidden => return None,
    })
}

fn deletion_input(backward: bool, primary: bool, word: bool) -> TextInputEvent {
    if cfg!(target_os = "macos") && primary {
        if backward {
            TextInputEvent::BackspaceToStart
        } else {
            TextInputEvent::DeleteToEnd
        }
    } else if word {
        if backward {
            TextInputEvent::BackspaceWord
        } else {
            TextInputEvent::DeleteWord
        }
    } else if backward {
        TextInputEvent::Backspace
    } else {
        TextInputEvent::Delete
    }
}

fn standard_shortcut(key: &crate::KeyboardKey, modifiers: crate::Modifiers) -> Option<CommandId> {
    use crate::standard_commands::*;
    let primary = crate::Shortcut::primary("").modifiers;
    let crate::KeyboardKey::Character(key) = key else {
        return None;
    };
    if key.eq_ignore_ascii_case("z")
        && modifiers
            == (crate::Modifiers {
                shift: true,
                ..primary
            })
    {
        return Some(CommandId::of::<Redo>());
    }
    if modifiers != primary {
        return None;
    }
    Some(match key.as_str() {
        "z" | "Z" => CommandId::of::<Undo>(),
        "y" | "Y" if !cfg!(target_os = "macos") => CommandId::of::<Redo>(),
        "c" | "C" => CommandId::of::<Copy>(),
        "x" | "X" => CommandId::of::<Cut>(),
        "v" | "V" => CommandId::of::<Paste>(),
        "a" | "A" => CommandId::of::<SelectAll>(),
        "w" | "W" => CommandId::of::<CloseWindow>(),
        "q" | "Q" => CommandId::of::<Quit>(),
        _ => return None,
    })
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
    fn font_registration_validates_now_and_queues_for_the_shared_painter() {
        let mut runtime = Runtime::new();
        let commands = attach(&mut runtime);
        let font: Arc<[u8]> = include_bytes!("../tests/fonts/SourceSans3-Regular.otf")
            .as_slice()
            .into();
        let faces = runtime.update(|cx| cx.register_font(font.clone())).unwrap();
        assert!(faces[0].families.iter().any(|f| f == "Source Sans 3"));
        assert!(matches!(
            runtime.update(|cx| cx.register_font(vec![1u8, 2, 3])),
            Err(ApplicationError::Ui(UiError::Text(_)))
        ));
        runtime
            .update(|cx| cx.map_font_family(crate::FontFamily::Monospace, "Source Sans 3"))
            .unwrap();
        let queue = commands.queue.borrow();
        assert_eq!(queue.len(), 2);
        assert!(matches!(
            &queue[0],
            Command::Font(FontChange::Load(bytes)) if Arc::ptr_eq(bytes, &font)
        ));
        assert!(matches!(
            &queue[1],
            Command::Font(FontChange::Map(crate::FontFamily::Monospace, target))
                if &**target == "Source Sans 3"
        ));
    }
    #[test]
    fn file_drop_hooks_preserve_paths_order_and_window_identity_without_a_ui() {
        let mut runtime = Runtime::new();
        let commands = attach(&mut runtime);
        let root = runtime.update(|cx| cx.new(|_| Counter(0)));
        let parent = runtime
            .update(|cx| cx.open_window(WindowOptions::new(), root.clone()))
            .unwrap();
        let other = runtime
            .update(|cx| cx.open_window(WindowOptions::new(), root.clone()))
            .unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let output = received.clone();
        let mut hook: Option<Box<FileDropHook>> = Some(Box::new(move |window, event, cx| {
            assert!(cx.window().is_none());
            output.borrow_mut().push((window.id(), event.clone()));
            root.update(cx, |state, _| state.0 += 1);
        }));
        let path = std::path::PathBuf::from("nonexistent/文書 with spaces.txt");
        let events = [
            WindowEvent::HoveredFile(path.clone()),
            WindowEvent::HoveredFileCancelled,
            WindowEvent::DroppedFile(path.clone()),
        ];
        for event in &events {
            let event = FileDropEvent::from_window_event(event).unwrap();
            deliver_file_drop(&mut runtime, &parent, &event, &mut hook);
        }
        // Drops need no preceding hover and each native file is a distinct delivery.
        deliver_file_drop(
            &mut runtime,
            &other,
            &FileDropEvent::Dropped(path.clone()),
            &mut hook,
        );
        assert_eq!(
            *received.borrow(),
            vec![
                (parent.id(), FileDropEvent::Hovered(path.clone())),
                (parent.id(), FileDropEvent::Cancelled),
                (parent.id(), FileDropEvent::Dropped(path.clone())),
                (other.id(), FileDropEvent::Dropped(path.clone()))
            ]
        );
        assert!(FileDropEvent::from_window_event(&WindowEvent::Focused(true)).is_none());
        runtime.update(|cx| cx.close_window(&parent).unwrap());
        deliver_file_drop(
            &mut runtime,
            &parent,
            &FileDropEvent::Dropped(path.clone()),
            &mut hook,
        );
        commands.exited.set(true);
        deliver_file_drop(
            &mut runtime,
            &other,
            &FileDropEvent::Dropped(path),
            &mut hook,
        );
        assert_eq!(received.borrow().len(), 4);
    }
    #[test]
    fn geometry_requests_validate_and_snapshots_survive_logical_close() {
        let mut runtime = Runtime::new();
        attach(&mut runtime);
        let root = runtime.update(|cx| cx.new(|_| Counter(0)));
        for geometry in [
            WindowGeometry::new(f64::NAN, 600.),
            WindowGeometry::new(800., 0.),
            WindowGeometry::new(f64::INFINITY, 600.),
        ] {
            assert!(matches!(
                runtime.update(
                    |cx| cx.open_window(WindowOptions::new().geometry(geometry), root.clone())
                ),
                Err(ApplicationError::InvalidWindowOptions)
            ));
        }
        let geometry = WindowGeometry {
            outer_position: Some([-900, 120]),
            maximized: true,
            ..WindowGeometry::new(800., 600.)
        };
        let window = runtime
            .update(|cx| cx.open_window(WindowOptions::new().geometry(geometry), root.clone()))
            .unwrap();
        assert_eq!(window.geometry(), Some(geometry));
        runtime.update(|cx| cx.close_window(&window).unwrap());
        assert_eq!(window.geometry(), Some(geometry));
        assert!(window.is_closed());
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
    fn freshly_reconciled_child_commands_keep_their_source_window() {
        struct RememberSource;
        impl crate::Command for RememberSource {}
        struct Child(Option<WindowId>);
        impl View for Child {
            fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
                crate::button("Remember").key("remember").on_command(
                    cx.command(RememberSource, |s, _, cx| s.0 = cx.window().map(|w| w.id())),
                )
            }
        }
        struct Parent(Entity<Child>);
        impl View for Parent {
            fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
                crate::column().child(self.0.clone())
            }
        }
        struct Measure;
        impl TextMeasure for Measure {
            fn measure(
                &mut self,
                _: ElementId,
                _: crate::TextRequest<'_>,
            ) -> Result<[f32; 2], UiError> {
                Ok([80., 20.])
            }
        }
        let mut r = Runtime::new();
        let commands = attach(&mut r);
        let (first, root, handle) = r.update(|cx| {
            let child = cx.new(|_| Child(None));
            let root = cx.new(|_| Parent(child.clone()));
            let handle = cx.open_window(WindowOptions::new(), root.clone()).unwrap();
            (child, root, handle)
        });
        let mut ui = Ui::new(&mut r, root.clone()).unwrap();
        for expected in [first.clone(), r.update(|cx| cx.new(|_| Child(None)))] {
            r.update(|cx| root.update(cx, |s, _| s.0 = expected.clone()));
            ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
            record_mounts(&commands, handle.id(), &ui);
            let target = ui
                .elements()
                .find(|e| e.key == Some(&crate::Key::from("remember")))
                .unwrap()
                .id;
            assert!(ui.focus(target));
            assert_eq!(
                ui.dispatch_command::<RememberSource>(&mut r).unwrap(),
                CommandStatus::Handled
            );
            assert_eq!(r.update(|cx| expected.read(cx).0), Some(handle.id()));
        }
        assert_eq!(r.update(|cx| first.read(cx).0), Some(handle.id()));
    }

    #[test]
    fn lifecycle_requests_are_vetoable_intents_and_validate_before_queueing() {
        let mut r = Runtime::new();
        assert!(matches!(
            r.update(|cx| cx.request_quit()),
            Err(ApplicationError::NoHost)
        ));
        let commands = attach(&mut r);
        let handle = r.update(|cx| {
            let e = cx.new(|_| Counter(0));
            cx.open_window(WindowOptions::new(), e).unwrap()
        });
        r.update(|cx| {
            cx.request_close(&handle).unwrap();
            cx.request_quit().unwrap();
        });
        assert!(!handle.is_closed());
        assert!(!commands.exited.get());
        assert!(matches!(
            commands.queue.borrow().back(),
            Some(Command::RequestQuit)
        ));
        let mut foreign = Runtime::new();
        attach(&mut foreign);
        assert!(matches!(
            foreign.update(|cx| cx.request_close(&handle)),
            Err(ApplicationError::Ui(UiError::Access(
                crate::AccessError::WrongRuntime
            )))
        ));
        r.update(|cx| cx.close_window(&handle)).unwrap();
        assert!(matches!(
            r.update(|cx| cx.request_close(&handle)),
            Err(ApplicationError::ClosedWindow)
        ));
        r.update(|cx| cx.exit()).unwrap();
        assert!(matches!(
            r.update(|cx| cx.request_quit()),
            Err(ApplicationError::Exited)
        ));
    }
    #[test]
    fn editing_shortcuts_include_replay_and_native_delete_units() {
        use crate::standard_commands::{Redo, Undo};
        for (shortcut, command) in [
            (crate::Shortcut::primary("z"), CommandId::of::<Undo>()),
            (
                crate::Shortcut::primary("z").shift(),
                CommandId::of::<Redo>(),
            ),
        ] {
            assert_eq!(
                standard_shortcut(&shortcut.key, shortcut.modifiers),
                Some(command)
            );
        }
        let y = crate::Shortcut::primary("y");
        assert_eq!(
            standard_shortcut(&y.key, y.modifiers),
            (!cfg!(target_os = "macos")).then_some(CommandId::of::<Redo>())
        );
        assert_eq!(
            deletion_input(true, false, true),
            TextInputEvent::BackspaceWord
        );
        assert_eq!(
            deletion_input(false, false, true),
            TextInputEvent::DeleteWord
        );
        assert_eq!(
            deletion_input(true, true, false),
            if cfg!(target_os = "macos") {
                TextInputEvent::BackspaceToStart
            } else {
                TextInputEvent::Backspace
            }
        );
    }
    #[test]
    fn click_sequences_reset_on_timeout_drag_distance_and_after_triple_click() {
        let mut clicks = ClickTracker::default();
        let now = Instant::now();
        assert_eq!(clicks.press(now, [0.; 2], None), 1);
        assert_eq!(
            clicks.press(now + Duration::from_millis(100), [1., 0.], None),
            2
        );
        assert_eq!(
            clicks.press(now + Duration::from_millis(200), [1., 0.], None),
            3
        );
        assert_eq!(
            clicks.press(now + Duration::from_millis(300), [1., 0.], None),
            1
        );
        assert_eq!(
            clicks.press(now + Duration::from_millis(1000), [1., 0.], None),
            1
        );
        clicks.motion([20., 0.]);
        assert_eq!(
            clicks.press(now + Duration::from_millis(1100), [1., 0.], None),
            1
        );
    }

    #[test]
    fn standard_shortcuts_require_exact_primary_modifiers() {
        let shortcut = crate::Shortcut::primary("C");
        assert_eq!(
            standard_shortcut(&shortcut.key, shortcut.modifiers),
            Some(CommandId::of::<crate::standard_commands::Copy>())
        );
        let shifted = shortcut.clone().shift();
        assert!(standard_shortcut(&shifted.key, shifted.modifiers).is_none());
        assert!(standard_shortcut(&shortcut.key, Default::default()).is_none());
    }

    fn preferences(scheme: ColorScheme) -> SystemPreferences {
        SystemPreferences {
            color_scheme: Some(scheme),
            ..Default::default()
        }
    }

    #[test]
    fn system_mode_follows_injected_preferences_while_explicit_themes_win() {
        let mut runtime = Runtime::new();
        let commands = attach(&mut runtime);
        let root = runtime.update(|cx| cx.new(|_| Counter(0)));
        runtime
            .update(|cx| cx.set_theme_mode(ThemeMode::System))
            .unwrap();
        let (inherited, explicit) = runtime.update(|cx| {
            (
                cx.open_window(WindowOptions::new(), root.clone()).unwrap(),
                cx.open_window(WindowOptions::new().theme(Theme::dark()), root.clone())
                    .unwrap(),
            )
        });
        // A fake source: every snapshot goes through the host's delivery path.
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let mut hook: Option<Box<PreferencesHook>> = Some(Box::new(move |p, cx| {
            assert!(cx.window().is_none());
            assert_eq!(cx.system_preferences().unwrap(), *p);
            output.borrow_mut().push(p.clone());
        }));
        let light = preferences(ColorScheme::Light);
        assert!(deliver_preferences(
            &mut runtime,
            &commands,
            light.clone(),
            &mut hook
        ));
        assert!(!deliver_preferences(
            &mut runtime,
            &commands,
            light.clone(),
            &mut hook
        ));
        assert_eq!(inherited.theme(), Theme::light());
        assert_eq!(explicit.theme(), Theme::dark());
        // System-following windows leave the title bar to the OS; others match.
        assert_eq!(commands.native_theme(&inherited.life), None);
        assert_eq!(
            commands.native_theme(&explicit.life),
            Some(NativeTheme::Dark)
        );
        let contrast = SystemPreferences {
            high_contrast: true,
            reduce_motion: true,
            ..light.clone()
        };
        assert!(!runtime.reduce_motion());
        deliver_preferences(&mut runtime, &commands, contrast.clone(), &mut hook);
        assert_eq!(inherited.theme(), Theme::high_contrast());
        assert_eq!(*seen.borrow(), vec![light.clone(), contrast.clone()]);
        // The OS reduce-motion setting drives animation; an application override
        // survives preference changes that leave it alone.
        assert!(runtime.reduce_motion());
        runtime.update(|cx| cx.set_reduce_motion(false));
        let accent = SystemPreferences {
            accent_color: Some([0.2, 0.4, 0.8, 1.]),
            ..contrast
        };
        deliver_preferences(&mut runtime, &commands, accent, &mut hook);
        assert!(!runtime.reduce_motion());

        // An explicit application theme stops following the system.
        runtime.update(|cx| cx.set_theme(Theme::dark())).unwrap();
        assert_eq!(runtime.update(|cx| cx.theme_mode()).unwrap(), None);
        deliver_preferences(&mut runtime, &commands, light.clone(), &mut hook);
        assert_eq!(inherited.theme(), Theme::dark());
        assert_eq!(
            commands.native_theme(&inherited.life),
            Some(NativeTheme::Dark)
        );
        // Modes resume it; Light ignores the dark system appearance.
        runtime
            .update(|cx| cx.set_theme_mode(ThemeMode::Light))
            .unwrap();
        deliver_preferences(
            &mut runtime,
            &commands,
            preferences(ColorScheme::Dark),
            &mut hook,
        );
        assert_eq!(inherited.theme(), Theme::light());
        runtime
            .update(|cx| cx.set_theme_set(ThemeSet::default().map(Theme::compact)))
            .unwrap();
        assert_eq!(inherited.theme(), Theme::light().compact());
        let invalid = ThemeSet {
            dark: Theme::dark().metrics(|m| m.font_size = 0.),
            ..ThemeSet::default()
        };
        assert!(runtime.update(|cx| cx.set_theme_set(invalid)).is_err());
        commands.exited.set(true);
        assert!(!deliver_preferences(
            &mut runtime,
            &commands,
            light,
            &mut hook
        ));
    }

    #[test]
    fn every_cursor_has_a_native_icon_except_hidden() {
        use crate::Cursor as C;
        use astrelis_winit::winit::window::CursorIcon as I;
        let all = [
            C::Default,
            C::Pointer,
            C::Text,
            C::Grab,
            C::Grabbing,
            C::ResizeHorizontal,
            C::ResizeVertical,
            C::Crosshair,
            C::NotAllowed,
            C::ResizeNorthSouth,
            C::ResizeEastWest,
            C::ResizeNorthEastSouthWest,
            C::ResizeNorthWestSouthEast,
            C::ResizeColumn,
            C::ResizeRow,
            C::Move,
            C::Wait,
            C::Progress,
            C::Help,
            C::ZoomIn,
            C::ZoomOut,
            C::ContextMenu,
            C::Copy,
            C::Alias,
            C::NoDrop,
            C::VerticalText,
            C::Cell,
        ];
        let icons: HashSet<_> = all.iter().map(|c| native_cursor(*c).unwrap()).collect();
        // Horizontal/Vertical are the column/row resize aliases.
        assert_eq!(icons.len(), all.len() - 2);
        assert_eq!(native_cursor(C::ResizeHorizontal), Some(I::ColResize));
        assert_eq!(
            native_cursor(C::ResizeNorthWestSouthEast),
            Some(I::NwseResize)
        );
        assert_eq!(native_cursor(C::Hidden), None);
    }

    #[test]
    fn window_chrome_options_validate_size_limits() {
        let mut runtime = Runtime::new();
        attach(&mut runtime);
        let root = runtime.update(|cx| cx.new(|_| Counter(0)));
        for options in [
            WindowOptions::new().min_size(f64::NAN, 10.),
            WindowOptions::new().max_size(0., 10.),
            WindowOptions::new()
                .min_size(400., 300.)
                .max_size(300., 300.),
        ] {
            assert!(matches!(
                runtime.update(|cx| cx.open_window(options, root.clone())),
                Err(ApplicationError::InvalidWindowOptions)
            ));
        }
        let options = WindowOptions::new()
            .min_size(200., 100.)
            .max_size(800., 600.)
            .transparent(true)
            .resizable(false)
            .always_on_top(true)
            .custom_title_bar();
        assert!(options.attributes.transparent);
        assert!(!options.attributes.resizable);
        assert_eq!(options.attributes.decorations, cfg!(target_os = "macos"));
        assert!(runtime.update(|cx| cx.open_window(options, root)).is_ok());
    }

    #[test]
    fn the_first_installed_platform_family_becomes_the_interface_font() {
        let candidates = crate::system_ui_font_families();
        let last = candidates[candidates.len() - 1];
        let loaded = ["Comic Sans", last].map(String::from);
        assert_eq!(system_ui_family(loaded.into_iter()).as_deref(), Some(last));
        assert_eq!(system_ui_family(std::iter::empty()), None);
    }

    #[test]
    #[ignore = "scans installed fonts; run on a desktop with -- --ignored"]
    fn a_platform_interface_font_is_installed() {
        let mut fonts = astrelis::TextSystem::new();
        fonts.load_system_fonts();
        let family = system_ui_family(fonts.fonts().flat_map(|f| f.families));
        assert!(
            family.is_some(),
            "none of {:?}",
            crate::system_ui_font_families()
        );
        eprintln!("interface font: {family:?}");
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
