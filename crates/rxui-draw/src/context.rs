use crate::{
    Error, Framebuffer, FramebufferOptions, Material, MaterialOptions, RenderTarget,
    SurfaceOptions, Texture, TextureOptions,
};
#[cfg(test)]
use crate::{Mesh, Vertex};

/// Asynchronous device failure reported without invoking wgpu's panic handler.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuEvent {
    /// The device cannot be used again. All device-owned resources must be rebuilt.
    DeviceLost {
        /// Backend reason, including explicit destruction.
        reason: wgpu::DeviceLostReason,
        /// Backend diagnostic.
        message: String,
    },
    /// Validation diagnostic outside an application error scope.
    Validation(String),
    /// Internal backend diagnostic outside an application error scope.
    Internal(String),
    /// The device exhausted memory. Continuing on this device is unsafe.
    OutOfMemory(String),
}
impl std::fmt::Display for GpuEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceLost { reason, message } => {
                write!(f, "device lost ({reason:?}): {message}")
            }
            Self::Validation(message) => write!(f, "GPU validation: {message}"),
            Self::Internal(message) => write!(f, "GPU internal error: {message}"),
            Self::OutOfMemory(message) => write!(f, "GPU out of memory: {message}"),
        }
    }
}
impl std::error::Error for GpuEvent {}

#[derive(Debug, Default)]
struct GpuEvents {
    // Bound diagnostics during repeated validation failures, preserving device loss.
    errors: std::collections::VecDeque<GpuEvent>,
    lost: Option<GpuEvent>,
}
impl GpuEvents {
    fn push(&mut self, event: GpuEvent) {
        log::error!("{event}");
        match event {
            GpuEvent::DeviceLost { .. } | GpuEvent::OutOfMemory(_) => self.lost = Some(event),
            _ if self.errors.len() < 64 => self.errors.push_back(event),
            _ => {}
        }
    }
}

/// Shared wgpu initialization and GPU resources for one or more render targets.
///
/// A context owns a [`wgpu::Instance`], a selected [`wgpu::Adapter`], and its
/// [`wgpu::Device`] and [`wgpu::Queue`]. The instance creates surfaces and finds
/// adapters; the device creates resources, and the queue uploads and submits work.
/// Each [`RenderTarget`] owns its own surface, size, and presentation configuration.
///
/// Start with [`Self::with_surface`] to select an adapter compatible with the first
/// window, then use [`Self::create_surface`] for additional windows. All targets,
/// meshes, and renderers using this context share the same device. Additional
/// surfaces must be supported by the selected adapter; a context does not select
/// another adapter automatically when a surface is incompatible.
///
/// Cloning shares all wgpu handles without creating another instance or device.
/// Default initialization requests no optional device features, including timestamp
/// queries. Use [`Self::from_wgpu`] for application-controlled device configuration.
#[derive(Clone, Debug)]
pub struct GraphicsContext {
    events: std::sync::Arc<std::sync::Mutex<GpuEvents>>,
    instance: Option<wgpu::Instance>,
    adapter: Option<wgpu::Adapter>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub(crate) submission_lock: std::sync::Arc<std::sync::Mutex<()>>,
    pub(crate) upload_pool: crate::uploads::UploadPool,
    device_lost: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) rounded_clip_capacity: usize,
    pub(crate) no_clip_stack: std::sync::Arc<std::sync::OnceLock<wgpu::BindGroup>>,
    pub(crate) clip_layout: std::sync::Arc<std::sync::OnceLock<wgpu::BindGroupLayout>>,
    no_clip: std::sync::Arc<std::sync::OnceLock<wgpu::Buffer>>,
}

impl GraphicsContext {
    /// Allocates fixed-capacity linear/ring samples for polylines and markers.
    /// Creation checks vertex storage/device limits and queues no sample uploads.
    /// Use replace/append/write explicitly before recording the consuming frame.
    pub fn create_point_buffer(
        &self,
        options: crate::PointBufferOptions,
    ) -> Result<crate::PointBuffer, Error> {
        crate::PointBuffer::create(self, options)
    }
    /// Validates and uploads an immutable solid, linear, or radial brush.
    /// Stops, geometry mapping, and device limits are checked before GPU allocation.
    /// Reuse it with paths, shapes, or lines on this device; draw color acts as tint.
    pub fn create_brush(&self, options: crate::BrushOptions<'_>) -> Result<crate::Brush, Error> {
        crate::Brush::create(self, options)
    }

    /// Immutable per-draw record that disables rounded clipping, shared by renderers.
    pub(crate) fn no_clip(&self) -> &wgpu::Buffer {
        self.no_clip.get_or_init(|| {
            use wgpu::util::DeviceExt;
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("RXUI drawing disabled clip"),
                    contents: bytemuck::bytes_of(&crate::clip::DISABLED),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        })
    }
    /// Whether managed resources can be shared between these contexts.
    /// Native contexts compare instance and device. Device-only imports share
    /// resources through cloned contexts; independent imports are conservatively distinct.
    pub fn same_device(&self, other: &Self) -> bool {
        match (&self.instance, &other.instance) {
            (Some(a), Some(b)) => a == b && self.device == other.device,
            // wgpu device equality alone is not globally unique across instances.
            // Device-only imports share managed resources through cloned contexts.
            (None, None) => std::sync::Arc::ptr_eq(&self.upload_pool, &other.upload_pool),
            _ => false,
        }
    }

    /// Creates a reusable single-mip 2D color texture with a default sampling view.
    ///
    /// Creation validates nonzero dimensions, uncompressed format support, enabled
    /// device features, and usages before allocation. Defaults use sRGB RGBA8 with
    /// sampling and upload usages. Upload pixels separately with [`Texture::write`].
    /// No queue work is submitted or waited for by this factory.
    pub fn create_texture(&self, options: TextureOptions) -> Result<Texture, Error> {
        Texture::create(self, options)
    }

    /// Initializes wgpu and a first surface, selecting a compatible adapter.
    ///
    /// Options select physical dimensions and the initial sample count. A zero
    /// dimension creates a suspended target. Window creation and event handling
    /// remain application-owned. Target creation validates MSAA and caches usable
    /// counts before returning; unsupported requests do not fall back implicitly.
    /// RXUI drawing-created instances honour wgpu environment options. Native GLES
    /// presentation requires [`Self::with_surface_instance`] with a display handle.
    /// Passing an owned handle such as `Arc<Window>` allows a `'static` target;
    /// passing a borrowed window ties the target's lifetime to that borrow.
    ///
    /// # Errors
    ///
    /// Returns an error if surface creation, adapter selection, device creation, or
    /// target configuration fails. Dimensions must fit the device's texture limit.
    /// Unsupported MSAA returns [`Error::UnsupportedSampleCount`].
    pub async fn with_surface<'window>(
        window: impl Into<wgpu::SurfaceTarget<'window>>,
        options: SurfaceOptions,
    ) -> Result<(Self, RenderTarget<'window>), Error> {
        Self::with_surface_instance(
            &wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env()),
            window,
            options,
        )
        .await
    }

    /// Initializes a surface on a caller-configured instance, selecting a compatible adapter.
    /// For native GLES presentation, supply an instance with its display handle.
    pub async fn with_surface_instance<'window>(
        instance: &wgpu::Instance,
        window: impl Into<wgpu::SurfaceTarget<'window>>,
        options: SurfaceOptions,
    ) -> Result<(Self, RenderTarget<'window>), Error> {
        let surface = instance
            .create_surface(window)
            .map_err(Error::CreateSurface)?;
        let graphics = Self::request(instance, Some(&surface)).await?;
        let target = RenderTarget::surface(&graphics, surface, options)?;
        Ok((graphics, target))
    }

    /// Initializes wgpu without a presentation surface.
    ///
    /// Useful for compute or offscreen GPU work through the raw device and queue.
    /// Surfaces can be added later with [`Self::create_surface`], but the selected
    /// adapter is not guaranteed to support them. Prefer [`Self::with_surface`]
    /// when the application will present to a window.
    ///
    /// # Errors
    ///
    /// Returns an error if no suitable adapter or device can be created.
    pub async fn headless() -> Result<Self, Error> {
        Self::request(
            &wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env()),
            None,
        )
        .await
    }

    /// Creates and configures another surface using this context's device.
    ///
    /// Targets have independent sizes and presentation state. A single renderer
    /// can render to any of them, reusing meshes and cached pipelines. Options use
    /// physical pixels; a zero dimension suspends the target. The initial sample
    /// count is validated for this surface's format and usable counts are cached.
    /// The returned target borrows only a borrowed window, not the context.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CreateSurface`] if wgpu cannot create the surface,
    /// [`Error::UnsupportedSurface`] if this adapter cannot present to it, or
    /// [`Error::InvalidTargetSize`] if the dimensions exceed the device limit, or
    /// [`Error::UnsupportedSampleCount`] if the initial MSAA request is unsupported.
    pub fn create_surface<'window>(
        &self,
        window: impl Into<wgpu::SurfaceTarget<'window>>,
        options: SurfaceOptions,
    ) -> Result<RenderTarget<'window>, Error> {
        let surface = self
            .instance()
            .create_surface(window)
            .map_err(Error::CreateSurface)?;
        RenderTarget::surface(self, surface, options)
    }

    /// Validates and uploads an immutable indexed triangle mesh on this device.
    ///
    /// Every three Uint32 indices describe a triangle. Vertex positions use clip
    /// space, and colors use linear RGB with straight alpha. Upload once and reuse
    /// the mesh across frames and renderers on this device.
    ///
    /// # Errors
    ///
    /// Rejects empty geometry, incomplete triangles, out-of-bounds indices,
    /// nonfinite vertex components, and data exceeding the device's buffer limits.
    #[cfg(test)]
    pub fn create_mesh(&self, vertices: &[Vertex], indices: &[u32]) -> Result<Mesh, Error> {
        Mesh::upload(self, vertices, indices)
    }

    /// Uploads geometry with explicit layouts, index width, topology, and update usages.
    #[cfg(test)]
    pub fn create_mesh_with_options(&self, options: crate::MeshOptions<'_>) -> Result<Mesh, Error> {
        Mesh::upload_options(self, options)
    }

    /// Imports application-owned buffer ranges. Raw handle device identity uses wgpu
    /// validation; imported index values remain caller-controlled. Layout, range,
    /// and usage metadata are checked here.
    #[cfg(test)]
    pub fn create_mesh_from_buffers(
        &self,
        streams: &[crate::MeshVertexBuffer<'_>],
        indices: Option<crate::MeshIndexBuffer<'_>>,
        topology: wgpu::PrimitiveTopology,
    ) -> Result<Mesh, Error> {
        Mesh::from_buffers(self, streams, indices, topology)
    }

    /// Creates a reusable offscreen framebuffer with optional MSAA and depth/stencil.
    ///
    /// Validates dimensions, enabled format features, output usages, and sample
    /// counts before allocating. Zero dimensions create a suspended resource.
    /// Defaults allow rendering and shader sampling; add `COPY_SRC` for readback.
    /// Usable sample counts are cached at creation. Unsupported requests return
    /// an error without choosing a fallback or creating GPU attachments.
    pub fn create_framebuffer(&self, options: FramebufferOptions) -> Result<Framebuffer, Error> {
        Framebuffer::create(self, options)
    }

    /// Creates an immutable mesh material from an application-created shader.
    ///
    /// The shader and binding layouts must belong to this device. The material
    /// retains their GPU handles and owns its entry-point names and render state.
    /// Uniform buffers, textures, bind groups, and their updates remain owned by
    /// the application. Shader inputs follow the declared vertex layouts, defaulting to [`crate::Vertex`].
    ///
    /// As with raw wgpu resource creation, invalid layouts use wgpu's error
    /// reporting. Shader interfaces are validated when a renderer first creates
    /// a pipeline; use the drawing renderer to do that before
    /// rendering. This factory does not record, submit, or wait for GPU work.
    pub fn create_material(&self, options: MaterialOptions<'_>) -> Material {
        Material::create(self, options)
    }

    /// Creates reusable textured-rectangle shader, blending, and depth/stencil settings.
    pub fn create_texture_material(
        &self,
        options: crate::TextureMaterialOptions<'_>,
    ) -> crate::TextureMaterial {
        crate::TextureMaterial::create(self, options)
    }

    /// Requests a device from an application-created wgpu instance.
    ///
    /// The context retains a clone of the instance for later surface creation.
    /// Pass `Some(&surface)` to select an adapter compatible with an existing
    /// surface created by this instance; `None` selects without presentation
    /// constraints. This is the initialization path for custom instance settings.
    /// Uses WGPU_POWER_PREF and, on native platforms, WGPU_ADAPTER_NAME.
    /// An unmatched explicit adapter name returns an error without a fallback.
    ///
    /// # Errors
    ///
    /// Returns an error if adapter selection or device creation fails.
    pub async fn request(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, Error> {
        let options = wgpu::RequestAdapterOptions {
            compatible_surface: surface,
            power_preference: wgpu::PowerPreference::from_env().unwrap_or_default(),
            ..Default::default()
        };
        #[cfg(not(target_arch = "wasm32"))]
        let selected = if let Ok(name) = std::env::var("WGPU_ADAPTER_NAME") {
            Some(
                instance
                    .enumerate_adapters(wgpu::Backends::all())
                    .await
                    .into_iter()
                    .find(|a| {
                        a.get_info()
                            .name
                            .to_lowercase()
                            .contains(&name.to_lowercase())
                            && surface.is_none_or(|s| a.is_surface_supported(s))
                    })
                    .ok_or(Error::RequestedAdapterNotFound(name))?,
            )
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let selected: Option<wgpu::Adapter> = None;
        let adapter = if let Some(adapter) = selected {
            adapter
        } else {
            instance
                .request_adapter(&options)
                .await
                .map_err(Error::Adapter)?
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("RXUI drawing device"),
                ..Default::default()
            })
            .await
            .map_err(Error::Device)?;
        Ok(Self::from_wgpu(instance.clone(), adapter, device, queue))
    }

    /// Adopts application-created wgpu handles, preserving their configuration.
    ///
    /// The caller must supply the instance that created the adapter, the adapter
    /// that created the device, and the queue returned with that device. Optional
    /// features and device limits remain under the application's control.
    /// Installs RXUI drawing diagnostics, replacing existing device error/loss callbacks.
    /// Application error scopes still take precedence for scoped errors.
    pub fn from_wgpu(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Self {
        let events = std::sync::Arc::new(std::sync::Mutex::new(GpuEvents::default()));
        let uncaptured = events.clone();
        device.on_uncaptured_error(std::sync::Arc::new(move |error| {
            let message = error.to_string();
            let event = match error {
                wgpu::Error::Validation { .. } => GpuEvent::Validation(message),
                wgpu::Error::Internal { .. } => GpuEvent::Internal(message),
                wgpu::Error::OutOfMemory { .. } => GpuEvent::OutOfMemory(message),
            };
            uncaptured
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(event);
        }));
        let device_lost = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = device_lost.clone();
        let lost = events.clone();
        device.set_device_lost_callback(move |reason, message| {
            flag.store(true, std::sync::atomic::Ordering::Release);
            lost.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(GpuEvent::DeviceLost { reason, message });
        });
        let rounded_clip_capacity = crate::clip::capacity(&device.limits());
        Self {
            rounded_clip_capacity,
            events,
            instance: Some(instance),
            adapter: Some(adapter),
            device,
            queue,
            upload_pool: Default::default(),
            submission_lock: Default::default(),
            no_clip: Default::default(),
            device_lost,
            no_clip_stack: Default::default(),
            clip_layout: Default::default(),
        }
    }

    /// Shares an existing device and queue without installing callbacks or requiring an adapter.
    /// Native surface management remains the caller's responsibility.
    pub fn from_device_queue(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            rounded_clip_capacity: crate::clip::capacity(&device.limits()),
            events: Default::default(),
            instance: None,
            adapter: None,
            device: device.clone(),
            queue: queue.clone(),
            upload_pool: Default::default(),
            submission_lock: Default::default(),
            no_clip: Default::default(),
            device_lost: Default::default(),
            no_clip_stack: Default::default(),
            clip_layout: Default::default(),
        }
    }

    pub(crate) fn format_features(
        &self,
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureFormatFeatures {
        let guaranteed = format.guaranteed_format_features(self.device.features());
        let Some(adapter) = &self.adapter else {
            return guaranteed;
        };
        let hardware = adapter.get_texture_format_features(format);
        let downlevel = !adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::WEBGPU_TEXTURE_FORMAT_SUPPORT);
        if self
            .device
            .features()
            .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
            || downlevel
        {
            hardware
        } else {
            wgpu::TextureFormatFeatures {
                allowed_usages: hardware.allowed_usages & guaranteed.allowed_usages,
                flags: hardware.flags & guaranteed.flags,
            }
        }
    }

    /// Installs an application-owned notification bridge for device loss.
    ///
    /// Marks this context and its clones lost before invoking `callback`. Forward
    /// the notification to your event loop (for example with a winit proxy), then
    /// request a replacement device and rebuild your resources there. The callback
    /// must not capture this context or its device: that creates an ownership cycle.
    /// It may run on a backend thread; do not perform window operations in it.
    ///
    /// Replaces default device-loss queue delivery with this application bridge, so
    /// the runner leaves recovery to your callback instead of exiting on that event.
    /// Uncaptured-error diagnostics (including fatal out-of-memory) remain enabled.
    /// On native backends this replaces wgpu's single device-loss callback. Replacing
    /// it through the raw device bypasses this tracking. Install once before GPU work;
    /// independently imported contexts do not share the tracking flag. No polling,
    /// redraw scheduling, device replacement, or resource rebuilding is performed.
    /// Native callbacks may be deferred until device polling; custom loops should
    /// use a nonblocking raw `device().poll(wgpu::PollType::Poll)` on GPU failures.
    pub fn set_device_lost_callback(
        &self,
        callback: impl Fn(wgpu::DeviceLostReason, String) + Send + 'static,
    ) {
        let lost = self.device_lost.clone();
        self.device
            .set_device_lost_callback(move |reason, message| {
                lost.store(true, std::sync::atomic::Ordering::Release);
                log::error!("device lost ({reason:?}): {message}");
                callback(reason, message);
            });
    }

    /// Whether the default or application device-loss callback fired.
    /// This is a permanent flag for this device, not a backend poll or health probe.
    pub fn is_device_lost(&self) -> bool {
        self.device_lost.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Drains pending diagnostics shared by all clones, with fatal diagnostics first.
    /// Custom loops should poll this regularly, including while presentation is idle.
    /// At most 64 uncaptured errors are retained; all errors are logged immediately.
    pub fn take_gpu_events(&self) -> Vec<GpuEvent> {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        let mut result: Vec<_> = events.lost.take().into_iter().collect();
        result.extend(events.errors.drain(..));
        result
    }

    /// Returns the wgpu instance for adapter discovery and custom surface creation.
    pub fn instance(&self) -> &wgpu::Instance {
        self.instance
            .as_ref()
            .expect("surface operations require a native graphics context")
    }

    /// Returns the selected wgpu adapter.
    pub fn adapter(&self) -> &wgpu::Adapter {
        self.adapter
            .as_ref()
            .expect("surface operations require a native graphics context")
    }

    /// Returns the shared wgpu device for custom resource creation and GPU work.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Returns the shared wgpu queue for uploads, submission, and callbacks.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn loss_callback_marks_all_clones_before_notification() {
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let clone = graphics.clone();
        let (send, receive) = std::sync::mpsc::channel();
        // Capture only the flag, never the device/context itself.
        let lost = graphics.device_lost.clone();
        graphics.set_device_lost_callback(move |reason, message| {
            send.send((
                reason,
                message,
                lost.load(std::sync::atomic::Ordering::Acquire),
            ))
            .unwrap();
        });
        assert!(graphics.same_device(&clone));
        assert!(!clone.is_device_lost());
        // Installing the loss bridge must preserve the remote uncaptured-error policy.
        let _invalid = graphics.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("RXUI drawing explicit loss bridge diagnostics"),
            size: 4,
            usage: wgpu::BufferUsages::empty(),
            mapped_at_creation: false,
        });
        graphics.device().poll(wgpu::PollType::Poll).unwrap();
        assert!(
            clone
                .take_gpu_events()
                .iter()
                .any(|e| matches!(e, GpuEvent::Validation(_)))
        );
        graphics.device().destroy();
        let _ = graphics.device().poll(wgpu::PollType::Poll);
        let (reason, _, already_lost) = receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(reason, wgpu::DeviceLostReason::Destroyed);
        assert!(already_lost);
        assert!(clone.is_device_lost());
        // The explicit bridge owns loss policy, while validation diagnostics remain.
        assert!(graphics.take_gpu_events().is_empty());
        let replacement = pollster::block_on(GraphicsContext::headless()).unwrap();
        assert!(!graphics.same_device(&replacement));
        assert!(!replacement.is_device_lost());
    }

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn required_metal_hardware_is_available() {
        if std::env::var_os("ASTRELIS_REQUIRE_METAL_GPU").is_none() {
            return;
        }
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let info = graphics.adapter().get_info();
        eprintln!("Hardware CI adapter: {info:?}");
        assert_eq!(info.backend, wgpu::Backend::Metal);
        assert!(matches!(
            info.device_type,
            wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu
        ));
    }

    #[test]
    fn diagnostics_are_bounded_and_reserve_fatal_delivery() {
        let mut events = GpuEvents::default();
        for _ in 0..100 {
            events.push(GpuEvent::Validation("bad frame".into()));
        }
        events.push(GpuEvent::OutOfMemory("allocation".into()));
        assert_eq!(events.errors.len(), 64);
        assert!(matches!(events.lost, Some(GpuEvent::OutOfMemory(_))));
        events.push(GpuEvent::DeviceLost {
            reason: wgpu::DeviceLostReason::Destroyed,
            message: "destroyed".into(),
        });
        assert!(matches!(events.lost, Some(GpuEvent::DeviceLost { .. })));
    }

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn uncaptured_validation_and_device_loss_are_reported_without_panicking() {
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let clone = graphics.clone();
            // Empty usages are invalid; this must use our callback rather than panic.
            let _invalid = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("RXUI drawing diagnostics test"),
                size: 4,
                usage: wgpu::BufferUsages::empty(),
                mapped_at_creation: false,
            });
            graphics.device().poll(wgpu::PollType::Poll).unwrap();
            assert!(
                clone
                    .take_gpu_events()
                    .iter()
                    .any(|e| matches!(e, GpuEvent::Validation(_)))
            );
            assert!(graphics.take_gpu_events().is_empty());
            graphics.device().destroy();
            graphics.device().poll(wgpu::PollType::Poll).unwrap();
            assert!(clone.is_device_lost());
            assert!(graphics.take_gpu_events().iter().any(|e| matches!(
                e,
                GpuEvent::DeviceLost {
                    reason: wgpu::DeviceLostReason::Destroyed,
                    ..
                }
            )));
        });
    }
}
