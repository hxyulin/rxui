//! Native window, GPU, accessibility, and event-loop hosting for RXUI entities.

#![warn(missing_docs)]

use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};

use astrelis_app::{App as NativeApp, AppContext};
#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
use astrelis_app::{Runtime, RuntimeConfig, RuntimeError};
use astrelis_compositor::{CompositionStats, Compositor, ViewOptions, ViewRenderTarget};
use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize, Size},
};
use astrelis_gpu::{
    CompositeAlphaMode, DeviceDescriptor, PowerPreference, PresentMode, RequestAdapterOptions,
    SurfaceConfiguration, SurfaceFrameStatus, SurfaceTarget, TextureUsages, TextureViewDescriptor,
};
use astrelis_paint::CompositorViewId;
use astrelis_paint_gpu::{ExternalImage, RenderStats, RenderTarget, Renderer, RendererOptions};
#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
use astrelis_platform::WindowId;
use astrelis_platform::{
    Clipboard, CursorIcon, DeviceId, ElementState, Key, Modifiers, PointerButton, ScrollDelta,
    Window, WindowAttributes, WindowEvent,
};
use astrelis_text::FontDatabase;
use rxui_core::{App, Context, Entity, FlushStats, Render};
use rxui_tree::{
    AccessibilityUpdate, ClipboardOperation, NodeId, SemanticAction, SemanticNode, UiInput,
};

// Shared by the native example and headless integration tests; not host API.
#[doc(hidden)]
pub mod workbench;

/// Adapter and logical-device preferences used when opening a GPU surface.
#[derive(Clone, Debug, Default)]
pub struct GraphicsContextOptions {
    /// Adapter power preference.
    pub power_preference: PowerPreference,
    /// Require a software or fallback adapter.
    pub force_fallback_adapter: bool,
    /// Logical-device features and limits.
    pub device: DeviceDescriptor,
}

/// Shared graphics entry point used to create native surfaces.
#[derive(Clone)]
pub struct GraphicsContext {
    instance: astrelis_gpu::Instance,
    options: GraphicsContextOptions,
    device: Arc<Mutex<Option<SharedDevice>>>,
}

#[derive(Clone)]
struct SharedDevice {
    adapter: astrelis_gpu::Adapter,
    device: astrelis_gpu::Device,
    queue: astrelis_gpu::Queue,
}

impl GraphicsContext {
    /// Creates graphics with default backend and device preferences.
    pub fn new() -> Self {
        Self::with_options(GraphicsContextOptions::default())
    }

    /// Creates graphics with explicit adapter and device preferences.
    pub fn with_options(options: GraphicsContextOptions) -> Self {
        Self {
            instance: astrelis_gpu_wgpu::create_instance(Default::default()),
            options,
            device: Arc::new(Mutex::new(None)),
        }
    }

    /// Wraps an application-configured backend-neutral instance.
    pub fn from_instance(instance: astrelis_gpu::Instance) -> Self {
        Self::from_instance_with_options(instance, GraphicsContextOptions::default())
    }

    /// Wraps an instance with explicit adapter and device preferences.
    pub fn from_instance_with_options(
        instance: astrelis_gpu::Instance,
        options: GraphicsContextOptions,
    ) -> Self {
        Self {
            instance,
            options,
            device: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the backend-neutral instance.
    pub const fn instance(&self) -> &astrelis_gpu::Instance {
        &self.instance
    }

    /// Returns adapter and device preferences.
    pub const fn options(&self) -> &GraphicsContextOptions {
        &self.options
    }

    async fn shared_device(
        &self,
        surface: astrelis_gpu::Surface,
    ) -> Result<SharedDevice, HostError> {
        if let Some(device) = self
            .device
            .lock()
            .expect("graphics context state poisoned")
            .clone()
        {
            return Ok(device);
        }
        let adapter = self
            .instance
            .request_adapter(RequestAdapterOptions {
                power_preference: self.options.power_preference,
                force_fallback_adapter: self.options.force_fallback_adapter,
                compatible_surface: Some(surface),
            })
            .await
            .map_err(HostError::from_display)?;
        let (device, queue) = adapter
            .request_device(self.options.device.clone())
            .await
            .map_err(HostError::from_display)?;
        let shared = SharedDevice {
            adapter,
            device,
            queue,
        };
        *self.device.lock().expect("graphics context state poisoned") = Some(shared.clone());
        Ok(shared)
    }
}

impl Default for GraphicsContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Creation and rendering policy for one entity window.
#[derive(Clone, Debug)]
pub struct WindowHostOptions {
    /// Platform window attributes.
    pub window: WindowAttributes,
    /// Color behind the RXUI display list.
    pub clear_color: Color,
    /// GPU painter configuration.
    pub renderer: RendererOptions,
}

impl Default for WindowHostOptions {
    fn default() -> Self {
        Self {
            window: WindowAttributes::default(),
            clear_color: Color::BLACK,
            renderer: RendererOptions::default(),
        }
    }
}

/// One platform accessibility action waiting for entity routing.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityRequest {
    /// Stable retained semantic target.
    pub target: NodeId,
    /// Backend-neutral semantic operation.
    pub action: SemanticAction,
}

/// Platform accessibility bridge hosted beside one entity application.
pub trait AccessibilityAdapter {
    /// Observes a platform event before RXUI input routing.
    fn handle_window_event(
        &mut self,
        _window: &Window,
        _event: &WindowEvent,
    ) -> Result<(), HostError> {
        Ok(())
    }

    /// Drains platform requests ready for entity routing.
    fn drain_requests(&mut self) -> Vec<AccessibilityRequest> {
        Vec::new()
    }

    /// Publishes one incremental semantic delta.
    fn update(&mut self, window: &Window, delta: &AccessibilityUpdate) -> Result<(), HostError>;

    /// Replaces adapter state with a complete semantic snapshot.
    fn reset(&mut self, window: &Window, snapshot: &[SemanticNode]) -> Result<(), HostError>;
}

/// Retained work scheduled from one platform event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetainedWork {
    /// Number of coalesced entity/retained passes scheduled by the event.
    pub passes: usize,
    /// Whether visible output may have changed.
    pub redraw: bool,
}

impl RetainedWork {
    /// Inspects an application before a host flush clears pending work.
    ///
    /// Effects are drained eagerly when `update_cell` and `new_entity` return
    /// to update depth zero, so a non-empty effect queue cannot hide redraw
    /// work when the host reads these observables.
    pub fn pending(app: &App, surface_reconfigured: bool) -> Self {
        Self {
            passes: usize::from(app.needs_flush()),
            redraw: surface_reconfigured || app.needs_render() || app.tree().needs_redraw(),
        }
    }
}

/// Observable GPU initialization state for a hosted window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostStatus {
    /// The surface, device, painter, and compositor are ready.
    Ready,
    /// The logical GPU device was lost.
    DeviceLost,
}

/// Result of routing one platform event through an entity window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostUpdate {
    /// The platform close button was requested.
    pub close_requested: bool,
    /// The window should request a frame.
    pub redraw: bool,
    /// Cursor, clipboard, or accessibility platform state changed.
    pub platform_state_changed: bool,
    /// Coalesced entity/retained scheduling decision.
    pub retained: RetainedWork,
}

struct GpuState {
    surface: astrelis_gpu::Surface,
    device: astrelis_gpu::Device,
    queue: astrelis_gpu::Queue,
    configuration: SurfaceConfiguration,
    render_format: astrelis_gpu::TextureFormat,
    compositor: Compositor,
}

/// An RXUI entity application connected to one native window and GPU surface.
pub struct WindowHost {
    window: Window,
    clipboard: Clipboard,
    gpu: Option<GpuState>,
    app: App,
    clear_color: Color,
    modifiers: Modifiers,
    pointer_positions: HashMap<DeviceId, LogicalPoint>,
    cursor_icon: CursorIcon,
    accessibility: Option<Box<dyn AccessibilityAdapter>>,
    last_work: RetainedWork,
}

impl WindowHost {
    /// Creates a platform window and connects it to an already-mounted app.
    pub fn open<A: NativeApp>(
        context: &mut AppContext<'_, '_, A>,
        graphics: &GraphicsContext,
        app: App,
        options: WindowHostOptions,
    ) -> Result<Self, HostError> {
        let window = context
            .create_window(options.window)
            .map_err(HostError::from_display)?;
        let clipboard = context.clipboard();
        let result = pollster::block_on(initialize_gpu(
            graphics.clone(),
            window.clone(),
            options.renderer,
        ));
        let gpu = match result {
            Ok(gpu) => gpu,
            Err(error) => {
                context.unregister_window(window.id());
                return Err(error);
            }
        };
        let mut host = Self {
            window,
            clipboard,
            gpu: Some(gpu),
            app,
            clear_color: options.clear_color,
            modifiers: Modifiers::default(),
            pointer_positions: HashMap::new(),
            cursor_icon: CursorIcon::Default,
            accessibility: None,
            last_work: RetainedWork::default(),
        };
        host.sync_viewport();
        host.flush_pending()?;
        Ok(host)
    }

    /// Returns the platform window.
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// Returns the hosted entity application.
    pub const fn app(&self) -> &App {
        &self.app
    }

    /// Returns mutable application access for explicit entity updates.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Returns the work scheduled by the most recently handled event or flush.
    pub const fn pending_work(&self) -> RetainedWork {
        self.last_work
    }

    /// Returns current GPU state.
    pub fn status(&self) -> HostStatus {
        if self.gpu.as_ref().is_some_and(|gpu| gpu.device.is_lost()) {
            HostStatus::DeviceLost
        } else {
            HostStatus::Ready
        }
    }

    /// Installs an accessibility adapter and publishes the current snapshot.
    pub fn set_accessibility_adapter(
        &mut self,
        mut adapter: impl AccessibilityAdapter + 'static,
    ) -> Result<(), HostError> {
        adapter.reset(&self.window, &self.app.tree().semantic_snapshot())?;
        self.accessibility = Some(Box::new(adapter));
        Ok(())
    }

    /// Removes and returns the accessibility adapter.
    pub fn take_accessibility_adapter(&mut self) -> Option<Box<dyn AccessibilityAdapter>> {
        self.accessibility.take()
    }

    /// Routes one platform event and coalesces all resulting work into one pass.
    pub fn handle_event(&mut self, event: &WindowEvent) -> Result<HostUpdate, HostError> {
        if let Some(accessibility) = &mut self.accessibility {
            accessibility.handle_window_event(&self.window, event)?;
        }
        if matches!(event, WindowEvent::CloseRequested) {
            self.last_work = RetainedWork::default();
            return Ok(HostUpdate {
                close_requested: true,
                ..HostUpdate::default()
            });
        }

        let mut surface_reconfigured = false;
        match event {
            WindowEvent::Resized(size) => {
                self.configure(size.width, size.height)?;
                surface_reconfigured = size.width != 0 && size.height != 0;
            }
            WindowEvent::ScaleFactorChanged { inner_size, .. } => {
                self.configure(inner_size.width, inner_size.height)?;
                surface_reconfigured = inner_size.width != 0 && inner_size.height != 0;
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = *modifiers,
            WindowEvent::PointerMoved {
                device_id,
                position,
            } => {
                let point = self.logical_point(position.x, position.y);
                self.pointer_positions.insert(*device_id, point);
                self.app.route_input(UiInput::PointerMoved(point));
            }
            WindowEvent::PointerLeft { device_id } => {
                self.pointer_positions.remove(device_id);
                self.app.route_input(UiInput::PointerLeft);
            }
            WindowEvent::PointerButton {
                device_id,
                button: PointerButton::Primary,
                state,
            } => {
                if let Some(point) = self.pointer_positions.get(device_id).copied() {
                    let input = match state {
                        ElementState::Pressed => UiInput::PointerPressed(point),
                        ElementState::Released => UiInput::PointerReleased(point),
                    };
                    self.app.route_input(input);
                }
            }
            WindowEvent::PointerWheel {
                device_id, delta, ..
            } => {
                if let Some(position) = self.pointer_positions.get(device_id).copied() {
                    let scale = self.window.scale_factor().max(f64::EPSILON);
                    let delta = match delta {
                        ScrollDelta::Lines { x, y } => LogicalPoint::new(-x * 40.0, -y * 40.0),
                        ScrollDelta::Pixels(point) => {
                            LogicalPoint::new((-point.x / scale) as f32, (-point.y / scale) as f32)
                        }
                    };
                    self.app
                        .route_input(UiInput::PointerWheel { position, delta });
                }
            }
            WindowEvent::KeyboardInput(input) => {
                let paste = input.state == ElementState::Pressed
                    && (self.modifiers.control || self.modifiers.super_key)
                    && matches!(
                        &input.logical_key,
                        Key::Character(value) if value.eq_ignore_ascii_case("v")
                    );
                self.app.route_input(UiInput::Keyboard {
                    input: input.clone(),
                    modifiers: self.modifiers,
                });
                if paste
                    && self.clipboard.capabilities().read_text
                    && let Some(text) = self
                        .clipboard
                        .read_text()
                        .map_err(HostError::from_display)?
                {
                    self.app.route_input(UiInput::Paste(text));
                }
            }
            WindowEvent::Ime(event) => self.app.route_input(UiInput::Ime(event.clone())),
            WindowEvent::Focused(focused) => {
                self.app.route_input(UiInput::FocusChanged(*focused));
            }
            _ => {}
        }

        let requests = self
            .accessibility
            .as_mut()
            .map(|adapter| adapter.drain_requests())
            .unwrap_or_default();
        let accessibility_requested = !requests.is_empty();
        for request in requests {
            self.app
                .route_semantic_action(request.target, request.action);
        }

        let work = RetainedWork::pending(&self.app, surface_reconfigured);
        let mut retained_redraw = false;
        consume_retained_work(
            work,
            || {
                let _ = self.app.flush();
                self.publish_accessibility()
            },
            || retained_redraw = true,
        )?;
        let clipboard_changed = self.flush_clipboard()?;
        let cursor = self.app.tree().cursor_icon();
        let cursor_changed = cursor != self.cursor_icon;
        if cursor_changed {
            self.window.set_cursor_icon(cursor);
            self.cursor_icon = cursor;
        }
        self.last_work = work;
        Ok(HostUpdate {
            close_requested: false,
            redraw: retained_redraw || accessibility_requested,
            platform_state_changed: clipboard_changed || cursor_changed || accessibility_requested,
            retained: work,
        })
    }

    /// Flushes application-owned mutations and returns their exact counters.
    pub fn flush(&mut self) -> Result<Option<FlushStats>, HostError> {
        self.flush_pending()
    }

    /// Generates and presents one UI-only frame.
    pub fn redraw(&mut self) -> Result<Option<RenderStats>, HostError> {
        self.redraw_composited(
            |_| ViewOptions::default(),
            |id, _, _| -> Result<(), HostError> {
                Err(HostError::new(format!(
                    "no scene callback was supplied for compositor view {}",
                    id.get()
                )))
            },
        )
        .map(|stats| stats.map(|stats| stats.paint))
    }

    /// Registers an application-owned texture sampled by retained paint.
    pub fn register_external_image(
        &mut self,
        image: &ExternalImage,
        view: astrelis_gpu::TextureView,
    ) -> Result<(), HostError> {
        self.ready_gpu()?
            .compositor
            .paint_mut()
            .register_external_image(image, view)
            .map_err(HostError::from_display)
    }

    /// Removes a previously registered retained-paint image.
    pub fn unregister_external_image(&mut self, image: &ExternalImage) -> bool {
        self.gpu
            .as_mut()
            .is_some_and(|gpu| gpu.compositor.paint_mut().unregister_external_image(image))
    }

    /// Presents a frame with application-rendered compositor views.
    pub fn redraw_composited<E>(
        &mut self,
        view_options: impl FnMut(CompositorViewId) -> ViewOptions,
        render_view: impl FnMut(
            CompositorViewId,
            &mut astrelis_gpu::CommandEncoder,
            ViewRenderTarget,
        ) -> Result<(), E>,
    ) -> Result<Option<CompositionStats>, HostError>
    where
        E: fmt::Display,
    {
        if self.gpu.is_none() {
            return Err(HostError::new("GPU initialization failed"));
        }
        if self.gpu.as_ref().is_some_and(|gpu| gpu.device.is_lost()) {
            return Err(HostError::new("the GPU device was lost; recreate the host"));
        }
        self.flush_pending()?;
        let list = self
            .app
            .tree()
            .scene()
            .flatten()
            .map_err(HostError::from_display)?;
        let scale_factor = self.window.scale_factor() as f32;
        let clear_color = self.clear_color;
        let gpu = self.ready_gpu()?;
        let frame = match gpu.surface.acquire().map_err(HostError::from_display)? {
            SurfaceFrameStatus::Ready(frame) | SurfaceFrameStatus::Suboptimal(frame) => frame,
            SurfaceFrameStatus::Outdated | SurfaceFrameStatus::Lost => {
                Self::reconfigure_gpu(gpu)?;
                return Ok(None);
            }
            SurfaceFrameStatus::Timeout | SurfaceFrameStatus::Occluded => return Ok(None),
            _ => return Ok(None),
        };
        let view = frame.texture().create_view(TextureViewDescriptor {
            format: Some(gpu.render_format),
            ..TextureViewDescriptor::default()
        });
        let mut encoder = gpu.device.create_command_encoder(Default::default());
        let stats = gpu
            .compositor
            .render(
                &mut encoder,
                &list,
                RenderTarget {
                    view,
                    format: gpu.render_format,
                    size: Size::new(gpu.configuration.width, gpu.configuration.height),
                    scale_factor,
                    clear_color,
                },
                view_options,
                render_view,
            )
            .map_err(HostError::from_display)?;
        gpu.queue
            .submit([encoder.finish().map_err(HostError::from_display)?])
            .map_err(HostError::from_display)?;
        frame.present().map_err(HostError::from_display)?;
        Ok(Some(stats))
    }

    fn flush_pending(&mut self) -> Result<Option<FlushStats>, HostError> {
        let work = RetainedWork::pending(&self.app, false);
        self.last_work = work;
        let mut stats = None;
        consume_retained_work(
            work,
            || {
                stats = Some(self.app.flush());
                self.publish_accessibility()?;
                self.flush_clipboard()?;
                Ok(())
            },
            || {},
        )?;
        Ok(stats)
    }

    fn publish_accessibility(&mut self) -> Result<(), HostError> {
        let delta = self.app.tree().accessibility_update();
        if let Some(accessibility) = &mut self.accessibility
            && (!delta.changed.is_empty() || !delta.removed.is_empty())
        {
            accessibility.update(&self.window, delta)?;
        }
        Ok(())
    }

    fn flush_clipboard(&mut self) -> Result<bool, HostError> {
        let mut changed = false;
        for operation in self.app.tree_mut().drain_clipboard() {
            match operation {
                ClipboardOperation::WriteText(text) if self.clipboard.capabilities().write_text => {
                    self.clipboard
                        .write_text(text)
                        .map_err(HostError::from_display)?;
                    changed = true;
                }
                ClipboardOperation::WriteText(_) => {}
            }
        }
        Ok(changed)
    }

    fn ready_gpu(&mut self) -> Result<&mut GpuState, HostError> {
        self.gpu
            .as_mut()
            .ok_or_else(|| HostError::new("GPU initialization failed"))
    }

    fn logical_point(&self, x: f64, y: f64) -> LogicalPoint {
        let scale = self.window.scale_factor().max(f64::EPSILON);
        LogicalPoint::new((x / scale) as f32, (y / scale) as f32)
    }

    fn configure(&mut self, width: u32, height: u32) -> Result<(), HostError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        if let Some(gpu) = &mut self.gpu {
            gpu.configuration.width = width;
            gpu.configuration.height = height;
            Self::reconfigure_gpu(gpu)?;
        }
        self.sync_viewport();
        Ok(())
    }

    fn reconfigure_gpu(gpu: &GpuState) -> Result<(), HostError> {
        gpu.surface
            .configure(&gpu.device, gpu.configuration.clone())
            .map_err(HostError::from_display)
    }

    fn sync_viewport(&mut self) {
        let scale = (self.window.scale_factor() as f32).max(f32::EPSILON);
        let size = self.window.inner_size().ok();
        let (width, height) = self
            .gpu
            .as_ref()
            .map(|gpu| (gpu.configuration.width, gpu.configuration.height))
            .or_else(|| size.map(|size| (size.width.max(1), size.height.max(1))))
            .unwrap_or((1, 1));
        self.app.tree_mut().set_viewport(LogicalSize::new(
            width as f32 / scale,
            height as f32 / scale,
        ));
    }
}

fn consume_retained_work<E>(
    work: RetainedWork,
    flush: impl FnOnce() -> Result<(), E>,
    redraw: impl FnOnce(),
) -> Result<(), E> {
    if work.passes != 0 {
        flush()?;
    }
    if work.redraw {
        redraw();
    }
    Ok(())
}

/// A typed root entity hosted in one native window.
pub struct EntityWindow<T: Render> {
    host: WindowHost,
    root: Entity<T>,
}

impl<T: Render> EntityWindow<T> {
    /// Creates, mounts, and opens a typed root entity.
    pub fn open<A: NativeApp>(
        context: &mut AppContext<'_, '_, A>,
        graphics: &GraphicsContext,
        initialize: impl FnOnce(&mut Context<'_, T>) -> T,
        options: WindowHostOptions,
    ) -> Result<Self, HostError> {
        let mut app = App::new(LogicalSize::new(1.0, 1.0), FontDatabase::default());
        let root = app.new_entity(initialize);
        let _ = app.mount(&root);
        let host = WindowHost::open(context, graphics, app, options)?;
        Ok(Self { host, root })
    }

    /// Returns the platform window.
    pub const fn window(&self) -> &Window {
        self.host.window()
    }

    /// Returns the low-level host.
    pub const fn host(&self) -> &WindowHost {
        &self.host
    }

    /// Returns mutable low-level host access.
    pub fn host_mut(&mut self) -> &mut WindowHost {
        &mut self.host
    }

    /// Returns the root entity handle.
    pub const fn entity(&self) -> &Entity<T> {
        &self.root
    }

    /// Routes one platform event.
    pub fn handle_event(&mut self, event: &WindowEvent) -> Result<HostUpdate, HostError> {
        self.host.handle_event(event)
    }

    /// Presents one frame.
    pub fn redraw(&mut self) -> Result<Option<RenderStats>, HostError> {
        self.host.redraw()
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
type EntityInitializer<T> = dyn FnOnce(&mut Context<'_, T>) -> T;

#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
struct EntityApplication<T: Render> {
    graphics: GraphicsContext,
    initialize: Option<Box<EntityInitializer<T>>>,
    options: Option<WindowHostOptions>,
    window: Option<EntityWindow<T>>,
    smoke: bool,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
impl<T: Render> EntityApplication<T> {
    fn new(
        initialize: impl FnOnce(&mut Context<'_, T>) -> T + 'static,
        options: WindowHostOptions,
    ) -> Self {
        Self {
            graphics: GraphicsContext::new(),
            initialize: Some(Box::new(initialize)),
            options: Some(options),
            window: None,
            smoke: std::env::var("RXUI_SMOKE").as_deref() == Ok("1"),
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
impl<T: Render> NativeApp for EntityApplication<T> {
    type Error = std::io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.window.is_none() {
            let initialize = self
                .initialize
                .take()
                .ok_or_else(|| std::io::Error::other("entity was already mounted"))?;
            let options = self
                .options
                .take()
                .ok_or_else(|| std::io::Error::other("window options are unavailable"))?;
            let window = EntityWindow::open(context, &self.graphics, initialize, options)
                .map_err(std::io::Error::other)?;
            context.invalidate_window(window.window().id());
            self.window = Some(window);
        }
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let Some(window) = &mut self.window else {
            return Ok(());
        };
        if window.window().id() != id {
            return Ok(());
        }
        let update = window.handle_event(&event).map_err(std::io::Error::other)?;
        if update.close_requested {
            self.window = None;
            context.unregister_window(id);
            context.exit();
        } else if update.redraw {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        let Some(window) = &mut self.window else {
            return Ok(());
        };
        if window.window().id() != id {
            return Ok(());
        }
        let rendered = window.redraw().map_err(std::io::Error::other)?;
        if self.smoke && rendered.is_some() {
            println!("RXUI_SMOKE rendered first frame");
            self.window = None;
            context.unregister_window(id);
            context.exit();
        } else if self.smoke {
            context.invalidate_window(id);
        }
        Ok(())
    }
}

/// Runs one root entity in a winit event loop.
///
/// With `RXUI_SMOKE=1`, the loop exits successfully after its first frame.
#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
pub fn run_entity<T: Render>(
    initialize: impl FnOnce(&mut Context<'_, T>) -> T + 'static,
    options: WindowHostOptions,
) -> Result<(), RuntimeError<std::io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        EntityApplication::new(initialize, options),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

async fn initialize_gpu(
    graphics: GraphicsContext,
    window: Window,
    renderer_options: RendererOptions,
) -> Result<GpuState, HostError> {
    let surface = graphics
        .instance
        .create_surface(SurfaceTarget::new(window.clone()))
        .map_err(HostError::from_display)?;
    let shared = graphics.shared_device(surface.clone()).await?;
    let adapter = shared.adapter;
    let device = shared.device;
    let queue = shared.queue;
    let capabilities = surface
        .capabilities(&adapter)
        .map_err(HostError::from_display)?;
    let format = capabilities
        .formats
        .first()
        .copied()
        .ok_or_else(|| HostError::new("surface reported no supported formats"))?;
    let size = window.inner_size().map_err(HostError::from_display)?;
    let render_format = srgb_view_format(format);
    let configuration = SurfaceConfiguration {
        usage: TextureUsages::RENDER_ATTACHMENT,
        format,
        view_formats: (render_format != format)
            .then_some(render_format)
            .into_iter()
            .collect(),
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: PresentMode::Fifo,
        alpha_mode: capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(CompositeAlphaMode::Opaque),
        desired_maximum_frame_latency: 2,
    };
    surface
        .configure(&device, configuration.clone())
        .map_err(HostError::from_display)?;
    let painter = Renderer::new(device.clone(), queue.clone(), renderer_options)
        .map_err(HostError::from_display)?;
    let compositor = Compositor::new(device.clone(), painter);
    Ok(GpuState {
        surface,
        device,
        queue,
        configuration,
        render_format,
        compositor,
    })
}

/// Stable failure returned by native hosting operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostError(String);

impl HostError {
    /// Creates an error from a stable diagnostic message.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn from_display(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for HostError {}

fn srgb_view_format(format: astrelis_gpu::TextureFormat) -> astrelis_gpu::TextureFormat {
    match format {
        astrelis_gpu::TextureFormat::Bgra8Unorm => astrelis_gpu::TextureFormat::Bgra8UnormSrgb,
        astrelis_gpu::TextureFormat::Rgba8Unorm => astrelis_gpu::TextureFormat::Rgba8UnormSrgb,
        _ => format,
    }
}

/// Native application-loop vocabulary used by custom hosts.
pub mod native {
    pub use astrelis_app::{App, AppContext, Runtime, RuntimeConfig, RuntimeError};
    pub use astrelis_platform::{Window, WindowAttributes, WindowEvent, WindowId};
    #[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
    pub use astrelis_platform_winit::run_return;
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use astrelis_core::geometry::{LogicalPoint, LogicalSize};
    use astrelis_text::FontDatabase;
    use rxui_core::{App, Context, Element, Render, label};
    use rxui_tree::{NodeId, SemanticAction, UiInput};

    use super::{RetainedWork, consume_retained_work, workbench};

    struct Idle;

    impl Render for Idle {
        fn render(&mut self, _context: &mut Context<Self>) -> Element {
            label("Idle")
        }
    }

    fn clean_app() -> App {
        let mut app = App::new(LogicalSize::new(320.0, 200.0), FontDatabase::empty());
        let root = app.new_entity(|_| Idle);
        let _ = app.mount(&root);
        app
    }

    fn workbench_app() -> App {
        let mut app = App::new(
            LogicalSize::new(workbench::VIEWPORT_WIDTH, workbench::VIEWPORT_HEIGHT),
            FontDatabase::empty(),
        );
        let root = app.new_entity(|_| workbench::Workbench::new());
        let _ = app.mount(&root);
        app
    }

    fn semantic_node(app: &App, label: &str) -> rxui_tree::SemanticNode {
        app.tree()
            .semantic_snapshot()
            .into_iter()
            .find(|node| node.data.label == label)
            .unwrap_or_else(|| panic!("missing semantic node {label:?}"))
    }

    fn semantic_node_id(app: &App, label: &str) -> NodeId {
        semantic_node(app, label).id
    }

    fn semantic_center(app: &App, label: &str) -> LogicalPoint {
        let bounds = semantic_node(app, label).bounds;
        LogicalPoint::new(
            bounds.origin.x + bounds.size.width * 0.5,
            bounds.origin.y + bounds.size.height * 0.5,
        )
    }

    #[test]
    fn idle_window_schedules_zero_passes() {
        let app = clean_app();
        assert_eq!(RetainedWork::pending(&app, false), RetainedWork::default());
    }

    #[test]
    fn surface_reconfiguration_schedules_redraw_without_a_retained_pass() {
        let app = clean_app();
        assert_eq!(
            RetainedWork::pending(&app, true),
            RetainedWork {
                passes: 0,
                redraw: true,
            }
        );
    }

    #[test]
    fn pointer_move_inside_the_same_workbench_target_schedules_nothing() {
        let mut app = workbench_app();
        let save = semantic_center(&app, "Save workspace");
        app.route_input(UiInput::PointerMoved(save));
        let _ = app.flush();

        app.route_input(UiInput::PointerMoved(LogicalPoint::new(
            save.x + 1.0,
            save.y,
        )));

        assert!(!app.needs_flush());
        assert!(!app.needs_render());
        assert!(!app.tree().needs_redraw());
        assert_eq!(RetainedWork::pending(&app, false), RetainedWork::default());
    }

    #[test]
    fn pointer_move_onto_save_schedules_a_pass_and_redraw() {
        let mut app = workbench_app();
        let save = semantic_center(&app, "Save workspace");

        app.route_input(UiInput::PointerMoved(save));

        assert!(app.needs_flush());
        assert!(!app.needs_render());
        assert!(app.tree().needs_redraw());
        assert_eq!(
            RetainedWork::pending(&app, false),
            RetainedWork {
                passes: 1,
                redraw: true,
            }
        );
    }

    #[test]
    fn flush_clears_workbench_pointer_work() {
        let mut app = workbench_app();
        let save = semantic_center(&app, "Save workspace");
        app.route_input(UiInput::PointerMoved(save));
        assert_ne!(RetainedWork::pending(&app, false), RetainedWork::default());

        let _ = app.flush();

        assert!(!app.needs_flush());
        assert!(!app.needs_render());
        assert!(!app.tree().needs_redraw());
        assert_eq!(RetainedWork::pending(&app, false), RetainedWork::default());
    }

    #[test]
    fn focusing_a_button_schedules_a_pass_without_a_frame() {
        // Focus on a button invalidates ACCESSIBILITY only (buttons have no
        // focus-indicator repaint), so this is the semantic-only change that
        // must publish without producing a frame.
        let mut app = workbench_app();
        let save = semantic_node_id(&app, "Save workspace");

        app.route_semantic_action(save, SemanticAction::Focus);

        assert!(app.needs_flush());
        assert!(!app.needs_render());
        assert!(!app.tree().needs_redraw());
        assert_eq!(
            RetainedWork::pending(&app, false),
            RetainedWork {
                passes: 1,
                redraw: false,
            }
        );
    }

    #[test]
    fn host_consumes_pass_and_redraw_decisions_independently() {
        let flushes = Cell::new(0);
        let redraws = Cell::new(0);
        consume_retained_work(
            RetainedWork {
                passes: 1,
                redraw: false,
            },
            || {
                flushes.set(flushes.get() + 1);
                Ok::<(), ()>(())
            },
            || redraws.set(redraws.get() + 1),
        )
        .expect("consume pass-only work");
        assert_eq!(flushes.get(), 1, "a scheduled pass must be flushed");
        assert_eq!(redraws.get(), 0, "pass-only work must not request a frame");

        consume_retained_work(
            RetainedWork {
                passes: 0,
                redraw: true,
            },
            || {
                flushes.set(flushes.get() + 1);
                Ok::<(), ()>(())
            },
            || redraws.set(redraws.get() + 1),
        )
        .expect("consume redraw-only work");
        assert_eq!(flushes.get(), 1, "zero-pass work must not be flushed");
        assert_eq!(redraws.get(), 1, "redraw work must request a frame");
    }
}
