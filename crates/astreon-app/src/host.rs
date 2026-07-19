//! A reusable Astrelis UI, surface, and painter host for one native window.

use std::{error::Error, fmt};

use astrelis_app::{App, AppContext};
use astrelis_core::{color::Color, geometry::Size};
use astrelis_gpu::{
    CompositeAlphaMode, DeviceDescriptor, PresentMode, RequestAdapterOptions, SurfaceConfiguration,
    SurfaceFrameStatus, SurfaceTarget, TextureUsages, TextureViewDescriptor,
};
use astrelis_paint_gpu::ExternalImage;
use astrelis_paint_gpu::{RenderStats, RenderTarget, Renderer, RendererOptions};
use astrelis_platform::{Window, WindowAttributes, WindowEvent, WindowId};
use astrelis_ui_core::Ui;

/// Shared graphics entry point used to open Astreon windows.
#[derive(Clone)]
pub struct GraphicsContext {
    instance: astrelis_gpu::Instance,
}

impl GraphicsContext {
    /// Creates graphics using Astrelis's default wgpu instance configuration.
    pub fn new() -> Self {
        Self {
            instance: astrelis_gpu_wgpu::create_instance(Default::default()),
        }
    }

    /// Wraps an application-configured Astrelis GPU instance.
    pub const fn from_instance(instance: astrelis_gpu::Instance) -> Self {
        Self { instance }
    }

    /// Returns the underlying backend-neutral instance.
    pub const fn instance(&self) -> &astrelis_gpu::Instance {
        &self.instance
    }
}

impl Default for GraphicsContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Creation and rendering policy for one UI window.
#[derive(Clone, Debug)]
pub struct WindowHostOptions {
    /// Native window attributes.
    pub window: WindowAttributes,
    /// Color used to clear pixels behind the UI display list.
    pub clear_color: Color,
    /// Painter renderer configuration.
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

struct GpuState {
    surface: astrelis_gpu::Surface,
    device: astrelis_gpu::Device,
    queue: astrelis_gpu::Queue,
    configuration: SurfaceConfiguration,
    renderer: Renderer,
}

/// Result of routing one native event through a window host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostUpdate {
    /// The native close button was requested.
    pub close_requested: bool,
    /// The window should be invalidated in the Astrelis runtime.
    pub redraw: bool,
    /// UI input changed cursor, IME, or another native window property.
    pub platform_state_changed: bool,
}

/// One retained UI tree connected to a native window and GPU surface.
pub struct WindowHost<Message = ()> {
    window: Window,
    gpu: GpuState,
    ui: Ui<Message>,
    clear_color: Color,
}

impl<Message: 'static> WindowHost<Message> {
    /// Creates, registers, and initializes a native window synchronously.
    ///
    /// GPU adapter initialization is blocked only during window creation. The
    /// desktop runtime remains idle-efficient after this call returns.
    pub fn open<A: App>(
        context: &mut AppContext<'_, '_, A>,
        graphics: &GraphicsContext,
        ui: Ui<Message>,
        options: WindowHostOptions,
    ) -> Result<Self, HostError> {
        let window = context
            .create_window(options.window)
            .map_err(HostError::from_display)?;
        let result = pollster::block_on(initialize_gpu(
            graphics.instance.clone(),
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
            gpu,
            ui,
            clear_color: options.clear_color,
        };
        host.sync_viewport();
        Ok(host)
    }

    /// Returns the native window identifier.
    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Returns the native window.
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// Returns the retained UI tree.
    pub const fn ui(&self) -> &Ui<Message> {
        &self.ui
    }

    /// Returns the retained UI tree for application updates.
    pub const fn ui_mut(&mut self) -> &mut Ui<Message> {
        &mut self.ui
    }

    /// Returns the backend-neutral GPU device used by this window.
    pub const fn device(&self) -> &astrelis_gpu::Device {
        &self.gpu.device
    }

    /// Returns the backend-neutral GPU queue used by this window.
    pub const fn queue(&self) -> &astrelis_gpu::Queue {
        &self.gpu.queue
    }

    /// Registers or replaces an application-owned texture sampled by a render view.
    pub fn register_external_image(
        &mut self,
        image: &ExternalImage,
        view: astrelis_gpu::TextureView,
    ) -> Result<(), HostError> {
        self.gpu
            .renderer
            .register_external_image(image, view)
            .map_err(HostError::from_display)
    }

    /// Removes a previously registered render-view image.
    pub fn unregister_external_image(&mut self, image: &ExternalImage) -> bool {
        self.gpu.renderer.unregister_external_image(image)
    }

    /// Drains typed messages emitted by UI listeners.
    pub fn drain_messages(&mut self) -> impl Iterator<Item = Message> + '_ {
        self.ui.drain_messages()
    }

    /// Routes one platform event, updates the surface, and reports scheduling work.
    pub fn handle_event(
        &mut self,
        clipboard: &astrelis_platform::Clipboard,
        event: &WindowEvent,
    ) -> Result<HostUpdate, HostError> {
        if matches!(event, WindowEvent::CloseRequested) {
            return Ok(HostUpdate {
                close_requested: true,
                ..Default::default()
            });
        }
        match event {
            WindowEvent::Resized(size) => self.configure(size.width, size.height)?,
            WindowEvent::ScaleFactorChanged { inner_size, .. } => {
                self.configure(inner_size.width, inner_size.height)?;
            }
            _ => {}
        }
        let update = self
            .ui
            .handle_window_event(&self.window, clipboard, event)
            .map_err(HostError::from_display)?;
        Ok(HostUpdate {
            close_requested: false,
            redraw: update.redraw || self.ui.needs_redraw(),
            platform_state_changed: update.platform_state_changed,
        })
    }

    /// Generates and presents the current UI display list.
    ///
    /// `None` means the surface was temporarily unavailable or occluded.
    pub fn redraw(&mut self) -> Result<Option<RenderStats>, HostError> {
        let list = self.ui.display_list().map_err(HostError::from_display)?;
        let frame = match self
            .gpu
            .surface
            .acquire()
            .map_err(HostError::from_display)?
        {
            SurfaceFrameStatus::Ready(frame) | SurfaceFrameStatus::Suboptimal(frame) => frame,
            SurfaceFrameStatus::Outdated | SurfaceFrameStatus::Lost => {
                self.reconfigure()?;
                return Ok(None);
            }
            SurfaceFrameStatus::Timeout | SurfaceFrameStatus::Occluded => return Ok(None),
            _ => return Ok(None),
        };
        let view = frame
            .texture()
            .create_view(TextureViewDescriptor::default());
        let mut encoder = self.gpu.device.create_command_encoder(Default::default());
        let stats = self
            .gpu
            .renderer
            .render(
                &mut encoder,
                &list,
                RenderTarget {
                    view,
                    format: self.gpu.configuration.format,
                    size: Size::new(self.gpu.configuration.width, self.gpu.configuration.height),
                    scale_factor: self.window.scale_factor() as f32,
                    clear_color: self.clear_color,
                },
            )
            .map_err(HostError::from_display)?;
        self.gpu
            .queue
            .submit([encoder.finish().map_err(HostError::from_display)?])
            .map_err(HostError::from_display)?;
        frame.present().map_err(HostError::from_display)?;
        Ok(Some(stats))
    }

    fn configure(&mut self, width: u32, height: u32) -> Result<(), HostError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.gpu.configuration.width = width;
        self.gpu.configuration.height = height;
        self.reconfigure()?;
        self.sync_viewport();
        Ok(())
    }

    fn reconfigure(&self) -> Result<(), HostError> {
        self.gpu
            .surface
            .configure(&self.gpu.device, self.gpu.configuration.clone())
            .map_err(HostError::from_display)
    }

    fn sync_viewport(&mut self) {
        let scale = (self.window.scale_factor() as f32).max(f32::EPSILON);
        self.ui.set_viewport(
            Size::new(
                self.gpu.configuration.width as f32 / scale,
                self.gpu.configuration.height as f32 / scale,
            ),
            scale,
        );
    }
}

async fn initialize_gpu(
    instance: astrelis_gpu::Instance,
    window: Window,
    renderer_options: RendererOptions,
) -> Result<GpuState, HostError> {
    let surface = instance
        .create_surface(SurfaceTarget::new(window.clone()))
        .map_err(HostError::from_display)?;
    let adapter = instance
        .request_adapter(RequestAdapterOptions {
            compatible_surface: Some(surface.clone()),
            ..Default::default()
        })
        .await
        .map_err(HostError::from_display)?;
    let (device, queue) = adapter
        .request_device(DeviceDescriptor::default())
        .await
        .map_err(HostError::from_display)?;
    let capabilities = surface
        .capabilities(&adapter)
        .map_err(HostError::from_display)?;
    let format = capabilities
        .formats
        .first()
        .copied()
        .ok_or_else(|| HostError::new("surface reported no supported formats"))?;
    let size = window.inner_size().map_err(HostError::from_display)?;
    let configuration = SurfaceConfiguration {
        usage: TextureUsages::RENDER_ATTACHMENT,
        format,
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
    let renderer = Renderer::new(device.clone(), queue.clone(), renderer_options)
        .map_err(HostError::from_display)?;
    Ok(GpuState {
        surface,
        device,
        queue,
        configuration,
        renderer,
    })
}

/// Failure while creating, updating, or rendering a hosted window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostError(String);

impl HostError {
    fn new(message: impl Into<String>) -> Self {
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
