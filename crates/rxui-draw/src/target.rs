use crate::{Error, Frame, FrameError, Framebuffer, GraphicsContext};

/// Initial physical size, MSAA, and optional depth/stencil for a window surface.
///
/// Pass to [`GraphicsContext::with_surface`], [`GraphicsContext::create_surface`],
/// or [`RenderTarget::surface`]. Constructing options performs no GPU work;
/// target creation validates them against the selected device and surface format.
/// Window creation and event handling remain application-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "pass these options to surface creation"]
pub struct SurfaceOptions {
    /// Initial width and height in physical pixels. Either being zero suspends rendering.
    pub size: [u32; 2],
    /// Color samples per pixel. One disables MSAA; unsupported counts are rejected.
    pub sample_count: u32,
    /// Optional depth-only, stencil-only, or combined attachment format.
    pub depth_stencil_format: Option<wgpu::TextureFormat>,
    /// Usages of depth/stencil storage, independent of color usages.
    /// Must include rendering; sampling or copies can be enabled when supported.
    pub depth_stencil_usage: wgpu::TextureUsages,
    /// Presentation pacing, default `Fifo` (vsync, supported everywhere). `AutoVsync`
    /// and `AutoNoVsync` let wgpu pick a supported mode; other modes must be offered by
    /// the surface.
    pub present_mode: wgpu::PresentMode,
    /// Desired queued frame count, clamped by wgpu to backend support. Default 2.
    /// Values below one are raised to one; one may serialize CPU and GPU work.
    pub desired_maximum_frame_latency: u32,
    /// How the compositor blends the window with what is behind it, default `Auto`
    /// (opaque where offered). Transparent windows need `PreMultiplied` or
    /// `PostMultiplied`; non-`Auto` modes must be offered by the surface.
    pub alpha_mode: wgpu::CompositeAlphaMode,
}

impl SurfaceOptions {
    /// Selects a physical size with MSAA disabled (one sample per pixel).
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            size: [width, height],
            sample_count: 1,
            depth_stencil_format: None,
            depth_stencil_usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
        }
    }

    /// Selects presentation pacing, for example `Mailbox` or `AutoNoVsync` for low latency.
    pub const fn present_mode(mut self, mode: wgpu::PresentMode) -> Self {
        self.present_mode = mode;
        self
    }

    /// Selects the desired queued frame count. Values below one become one.
    pub const fn desired_maximum_frame_latency(mut self, latency: u32) -> Self {
        self.desired_maximum_frame_latency = latency;
        self
    }

    /// Selects window compositing alpha, for example `PreMultiplied` for transparency.
    pub const fn alpha_mode(mut self, mode: wgpu::CompositeAlphaMode) -> Self {
        self.alpha_mode = mode;
        self
    }

    /// Enables a target-owned depth/stencil attachment with rendering usages.
    ///
    /// Creation validates the format and intersects color and depth/stencil sample
    /// support. Storage automatically follows target size and MSAA changes.
    /// No depth/stencil texture is allocated when this option is absent.
    pub const fn depth_stencil(mut self, format: wgpu::TextureFormat) -> Self {
        self.depth_stencil_format = Some(format);
        self
    }

    /// Replaces depth/stencil usages. Include rendering and desired sampling/copy usages.
    pub const fn depth_stencil_usage(mut self, usage: wgpu::TextureUsages) -> Self {
        self.depth_stencil_usage = usage;
        self
    }

    /// Selects the initial number of color samples per pixel.
    ///
    /// Target creation returns [`Error::UnsupportedSampleCount`] for unsupported
    /// counts without choosing a fallback. Zero-sized targets still validate the
    /// count, but defer attachment allocation until a nonzero resize.
    /// After creation, use [`RenderTarget::set_sample_count`] between frames to
    /// change MSAA without recreating the surface.
    pub const fn sample_count(mut self, count: u32) -> Self {
        self.sample_count = count;
        self
    }
}

/// A window surface or owned offscreen framebuffer used as a default frame destination.
#[derive(Debug)]
pub enum RenderTarget<'window> {
    /// An owned surface with managed physical size and presentation configuration.
    Surface(SurfaceTarget<'window>),
    /// A persistent offscreen color texture, optionally with a multisampled attachment.
    Framebuffer(Framebuffer),
}

/// Opaque state carried by [`RenderTarget::Surface`].
///
/// Construct and resize it through [`RenderTarget`]. Window creation and event
/// handling remain application-owned; this type has no winit dependency.
#[derive(Debug)]
pub struct SurfaceTarget<'window> {
    pub(crate) surface: wgpu::Surface<'window>,
    pub(crate) graphics: GraphicsContext,
    pub(crate) configuration: wgpu::SurfaceConfiguration,
    // Render view format: the configured format or its sRGB view format.
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) size: [u32; 2],
    pub(crate) sample_count: u32,
    pub(crate) multisample_view: Option<wgpu::TextureView>,
    supported_sample_counts: Vec<u32>,
    pub(crate) depth_stencil_format: Option<wgpu::TextureFormat>,
    depth_stencil_usage: wgpu::TextureUsages,
    pub(crate) depth_stencil: Option<crate::depth_stencil::DepthStencilAttachment>,
    // Whether the current configuration was already reapplied for a suboptimal image.
    suboptimal_reconfigured: bool,
}

impl<'window> RenderTarget<'window> {
    /// Takes ownership of a wgpu surface and configures it with the requested present
    /// and alpha modes (FIFO and automatic alpha by default).
    ///
    /// Options use physical pixels. A zero dimension suspends rendering
    /// until a nonzero resize. Rendering targets an sRGB format so that linear shader
    /// output is encoded for display: an sRGB surface format when one is offered,
    /// otherwise an sRGB view of a linear surface format (for example a WebGPU
    /// canvas offering only `Bgra8Unorm`) when the backend supports surface view
    /// formats. [`Self::format`] reports the view format that passes render to.
    /// Only when neither is available does output stay in the linear surface format.
    /// The initial sample count is validated and its attachment allocated before
    /// returning, except when suspended. Usable counts are queried once and cached.
    /// The surface must have been created by the context's instance. The
    /// application must not configure the same surface concurrently.
    /// Prefer [`GraphicsContext::create_surface`] for normal window initialization.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidTargetSize`] for dimensions exceeding device limits,
    /// [`Error::UnsupportedSurface`] for an incompatible surface,
    /// [`Error::UnsupportedPresentation`] for a present/alpha mode the surface lacks, or
    /// [`Error::UnsupportedSampleCount`] if the format cannot support the requested
    /// count with this device. No fallback is chosen or surface configured on rejection.
    pub fn surface(
        graphics: &GraphicsContext,
        surface: wgpu::Surface<'window>,
        options: SurfaceOptions,
    ) -> Result<Self, Error> {
        let SurfaceOptions {
            size: [width, height],
            sample_count,
            depth_stencil_format,
            depth_stencil_usage,
            present_mode,
            desired_maximum_frame_latency,
            alpha_mode,
        } = options;
        validate_size(graphics, width, height)?;
        if !graphics.adapter().is_surface_supported(&surface) {
            return Err(Error::UnsupportedSurface);
        }
        let capabilities = surface.get_capabilities(graphics.adapter());
        let mut configuration = surface
            .get_default_config(graphics.adapter(), width.max(1), height.max(1))
            .ok_or(Error::UnsupportedSurface)?;
        validate_presentation(&capabilities, present_mode, alpha_mode)?;
        configuration.desired_maximum_frame_latency = desired_maximum_frame_latency.max(1);
        let view_formats = graphics
            .adapter()
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        let format =
            if let Some((format, view)) = select_formats(&capabilities.formats, view_formats) {
                configuration.format = format;
                if view != format {
                    configuration.view_formats = vec![view];
                }
                view
            } else {
                configuration.format
            };
        configuration.present_mode = present_mode;
        configuration.alpha_mode = alpha_mode;
        let mut supported_sample_counts = supported_sample_counts(graphics, format);
        crate::depth_stencil::restrict_counts(
            graphics,
            &mut supported_sample_counts,
            depth_stencil_format,
            depth_stencil_usage,
        )?;
        validate_sample_count(format, sample_count, &supported_sample_counts)?;
        let multisample_view = create_multisample_view(
            graphics,
            configuration.format,
            format,
            [width, height],
            sample_count,
        );
        let mut target = SurfaceTarget {
            surface,
            graphics: graphics.clone(),
            configuration,
            format,
            size: [width, height],
            sample_count,
            multisample_view,
            supported_sample_counts,
            depth_stencil_format,
            depth_stencil_usage,
            depth_stencil: crate::depth_stencil::allocate(
                graphics,
                [width, height],
                sample_count,
                depth_stencil_format,
                depth_stencil_usage,
            ),
            suboptimal_reconfigured: false,
        };
        if width != 0 && height != 0 {
            target.configure();
        }
        Ok(Self::Surface(target))
    }

    /// Starts a frame for this destination without borrowing any renderer.
    ///
    /// The ready frame exclusively borrows this target until presentation or drop.
    /// Zero-sized or occluded targets return [`FrameError::Suspended`]. An
    /// outdated surface is reconfigured and acquisition retried once. Timeouts and
    /// surfaces that remain outdated return [`FrameError::Retry`].
    /// Framebuffers only suspend at zero size and have no surface acquisition step.
    ///
    /// # Errors
    ///
    /// [`FrameError::SurfaceLost`] requires replacing the target with a new surface from
    /// the owning window. [`FrameError::Validation`] indicates a wgpu acquisition
    /// validation failure.
    pub fn begin_frame(&mut self) -> Result<Frame<'_, 'window>, FrameError> {
        profiling::scope!("rxui_draw::begin_frame");
        let target = match self {
            Self::Surface(target) => target,
            Self::Framebuffer(framebuffer) => return framebuffer.begin_recording(),
        };
        if target.size.contains(&0) {
            return Err(FrameError::Suspended);
        }
        let mut acquisition = target.surface.get_current_texture();
        if matches!(acquisition, wgpu::CurrentSurfaceTexture::Outdated) {
            target.configure();
            acquisition = target.surface.get_current_texture();
        }
        let (image, suboptimal) = match acquisition {
            wgpu::CurrentSurfaceTexture::Success(image) => {
                target.suboptimal_reconfigured = false;
                (image, false)
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(image) => (image, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Outdated => {
                return Err(FrameError::Retry);
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Err(FrameError::Suspended),
            wgpu::CurrentSurfaceTexture::Lost => return Err(FrameError::SurfaceLost),
            wgpu::CurrentSurfaceTexture::Validation => return Err(FrameError::Validation),
        };
        Ok(Frame::new(target, image, suboptimal))
    }

    /// Updates physical size, replacing optional depth/stencil and MSAA storage.
    ///
    /// Unchanged dimensions reuse attachments. Changed sizes discard contents;
    /// depth/stencil loads require a new clear. Only nonzero surfaces are configured.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Error> {
        let target = match self {
            Self::Surface(target) => target,
            Self::Framebuffer(framebuffer) => return framebuffer.resize(width, height),
        };
        validate_size(&target.graphics, width, height)?;
        if target.size == [width, height] {
            return Ok(());
        }
        target.size = [width, height];
        target.multisample_view = create_multisample_view(
            &target.graphics,
            target.configuration.format,
            target.format,
            target.size,
            target.sample_count,
        );
        target.depth_stencil = crate::depth_stencil::allocate(
            &target.graphics,
            target.size,
            target.sample_count,
            target.depth_stencil_format,
            target.depth_stencil_usage,
        );
        if width != 0 && height != 0 {
            target.configuration.width = width;
            target.configuration.height = height;
            target.configure();
        }
        Ok(())
    }

    pub(crate) fn graphics(&self) -> &GraphicsContext {
        match self {
            Self::Surface(target) => &target.graphics,
            Self::Framebuffer(target) => &target.graphics,
        }
    }

    /// Returns the current physical dimensions, including zero while suspended.
    pub fn size(&self) -> [u32; 2] {
        match self {
            Self::Surface(target) => target.size,
            Self::Framebuffer(target) => target.size(),
        }
    }

    /// Returns the selected color format.
    pub fn format(&self) -> wgpu::TextureFormat {
        match self {
            Self::Surface(target) => target.format,
            Self::Framebuffer(target) => target.format(),
        }
    }

    /// Returns sample counts usable by all of this target's attachments.
    ///
    /// Counts are ascending and include one, which disables MSAA. Multisampling
    /// requires color render/resolve support and matching depth/stencil sample
    /// support when present; depth/stencil is stored without resolving. Adapter
    /// capabilities are restricted by the device's enabled features: native-only
    /// counts may require [`wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`]
    /// enabled through application-owned device initialization.
    /// This borrows the counts cached during creation; it performs no allocation
    /// or GPU capability query. Resizing and changing MSAA leave the cache valid.
    pub fn supported_sample_counts(&self) -> &[u32] {
        match self {
            Self::Surface(target) => &target.supported_sample_counts,
            Self::Framebuffer(target) => target.supported_sample_counts(),
        }
    }

    /// Returns the complete attachment format for pipeline preparation.
    pub fn render_format(&self) -> crate::RenderFormat {
        crate::RenderFormat {
            colors: vec![Some(self.format())],
            depth_stencil: self.depth_stencil_format(),
            sample_count: self.sample_count(),
        }
    }

    /// Returns the configured surface present mode, or `None` for a framebuffer.
    pub fn present_mode(&self) -> Option<wgpu::PresentMode> {
        match self {
            Self::Surface(target) => Some(target.configuration.present_mode),
            Self::Framebuffer(_) => None,
        }
    }

    /// Returns the configured surface alpha mode, or `None` for a framebuffer.
    pub fn alpha_mode(&self) -> Option<wgpu::CompositeAlphaMode> {
        match self {
            Self::Surface(target) => Some(target.configuration.alpha_mode),
            Self::Framebuffer(_) => None,
        }
    }

    /// Returns the current color sample count, initially selected by [`SurfaceOptions`].
    pub fn sample_count(&self) -> u32 {
        match self {
            Self::Surface(target) => target.sample_count,
            Self::Framebuffer(target) => target.sample_count(),
        }
    }

    /// Selects MSAA for future frames; one disables it.
    ///
    /// Call between frames. The target owns and reuses a multisampled color
    /// attachment matching its physical size and format. Each pass resolves into
    /// the single-sampled surface image, and load passes preserve the multisampled
    /// contents within the frame. Changing this setting recreates the attachment;
    /// selecting the current count does nothing. Zero-sized targets defer allocation
    /// until resized. The built-in mesh renderer selects matching pipelines automatically.
    /// Validation uses cached capabilities without querying the adapter or allocating
    /// a list. A changed count may allocate a texture; its first mesh draw may create
    /// a pipeline. Changing MSAA does not reconfigure the presentation surface.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedSampleCount`] without changing the target if
    /// the count is absent from [`Self::supported_sample_counts`]. No fallback is
    /// chosen implicitly.
    pub fn set_sample_count(&mut self, count: u32) -> Result<(), Error> {
        let target = match self {
            Self::Surface(target) => target,
            Self::Framebuffer(framebuffer) => return framebuffer.set_sample_count(count),
        };
        let format = target.format;
        if target.sample_count == count {
            return Ok(());
        }
        validate_sample_count(format, count, &target.supported_sample_counts)?;
        target.multisample_view = create_multisample_view(
            &target.graphics,
            target.configuration.format,
            format,
            target.size,
            count,
        );
        target.depth_stencil = crate::depth_stencil::allocate(
            &target.graphics,
            target.size,
            count,
            target.depth_stencil_format,
            target.depth_stencil_usage,
        );
        target.sample_count = count;
        Ok(())
    }

    /// Returns the optional managed depth/stencil format, including while suspended.
    pub fn depth_stencil_format(&self) -> Option<wgpu::TextureFormat> {
        match self {
            Self::Surface(target) => target.depth_stencil_format,
            Self::Framebuffer(target) => target.depth_stencil_format(),
        }
    }

    /// Returns configured depth/stencil texture usages, or `None` when disabled.
    pub fn depth_stencil_usage(&self) -> Option<wgpu::TextureUsages> {
        match self {
            Self::Surface(target) => target
                .depth_stencil_format
                .map(|_| target.depth_stencil_usage),
            Self::Framebuffer(target) => target.depth_stencil_usage(),
        }
    }

    /// Borrows managed depth/stencil storage for raw GPU integration.
    ///
    /// Returns `None` when disabled and `TargetSuspended` when enabled at zero size.
    /// Usages determine whether sampling/copies are permitted. MSAA is never
    /// resolved for this attachment. Resize/MSAA changes invalidate old handles;
    /// raw writes do not update wrapped initialization tracking.
    pub fn depth_stencil_texture(&self) -> Result<Option<&wgpu::Texture>, Error> {
        match self {
            Self::Surface(target) => {
                if target.depth_stencil_format.is_none() {
                    return Ok(None);
                }
                Ok(Some(
                    &target
                        .depth_stencil
                        .as_ref()
                        .ok_or(Error::TargetSuspended)?
                        .texture,
                ))
            }
            Self::Framebuffer(target) => target.depth_stencil_texture(),
        }
    }

    /// Borrows the depth/stencil attachment view, with the same rules as its texture.
    pub fn depth_stencil_view(&self) -> Result<Option<&wgpu::TextureView>, Error> {
        match self {
            Self::Surface(target) => {
                if target.depth_stencil_format.is_none() {
                    return Ok(None);
                }
                Ok(Some(
                    &target
                        .depth_stencil
                        .as_ref()
                        .ok_or(Error::TargetSuspended)?
                        .view,
                ))
            }
            Self::Framebuffer(target) => target.depth_stencil_view(),
        }
    }

    /// Returns the owned wgpu surface, or `None` for an offscreen destination.
    ///
    /// Configure and acquire through RXUI drawing while using [`Self::begin_frame`].
    /// External reconfiguration would invalidate the target's managed state.
    pub fn as_surface(&self) -> Option<&wgpu::Surface<'window>> {
        match self {
            Self::Surface(target) => Some(&target.surface),
            Self::Framebuffer(_) => None,
        }
    }

    /// Returns the framebuffer resource when this is an offscreen destination.
    pub fn as_framebuffer(&self) -> Option<&Framebuffer> {
        match self {
            Self::Surface(_) => None,
            Self::Framebuffer(target) => Some(target),
        }
    }

    /// Mutably borrows an offscreen destination for rendering in another frame.
    pub fn as_framebuffer_mut(&mut self) -> Option<&mut Framebuffer> {
        match self {
            Self::Surface(_) => None,
            Self::Framebuffer(target) => Some(target),
        }
    }
}

pub(crate) fn validate_sample_count(
    format: wgpu::TextureFormat,
    count: u32,
    supported: &[u32],
) -> Result<(), Error> {
    if !supported.contains(&count) {
        return Err(Error::UnsupportedSampleCount { format, count });
    }
    Ok(())
}

pub(crate) fn supported_sample_counts(
    graphics: &GraphicsContext,
    format: wgpu::TextureFormat,
) -> Vec<u32> {
    sample_counts(format_features(graphics, format))
}

pub(crate) fn format_features(
    graphics: &GraphicsContext,
    format: wgpu::TextureFormat,
) -> wgpu::TextureFormatFeatures {
    graphics.format_features(format)
}

pub(crate) fn sample_counts(usable: wgpu::TextureFormatFeatures) -> Vec<u32> {
    if !usable
        .allowed_usages
        .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
    {
        return Vec::new();
    }
    usable
        .flags
        .supported_sample_counts()
        .into_iter()
        .filter(|&count| {
            count == 1
                || usable
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE)
        })
        .collect()
}

pub(crate) fn create_multisample_view(
    graphics: &GraphicsContext,
    format: wgpu::TextureFormat,
    view_format: wgpu::TextureFormat,
    [width, height]: [u32; 2],
    sample_count: u32,
) -> Option<wgpu::TextureView> {
    if sample_count == 1 || width == 0 || height == 0 {
        return None;
    }
    let texture = graphics.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("RXUI drawing multisampled color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        // Match the resolve destination's storage and view formats. WebGPU
        // canvases use linear storage with sRGB views. Mixing sRGB MSAA storage
        // with a linear resolve texture can lose sRGB encoding in browser backends.
        view_formats: if format == view_format {
            &[]
        } else {
            std::slice::from_ref(&view_format)
        },
    });
    Some(texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(view_format),
        ..Default::default()
    }))
}

impl SurfaceTarget<'_> {
    pub(crate) fn configure(&mut self) {
        self.surface
            .configure(self.graphics.device(), &self.configuration);
        self.suboptimal_reconfigured = false;
    }

    /// Reapplies the configuration after presenting a suboptimal image, at most once
    /// until the configuration changes or an optimal image is acquired. Some
    /// platforms keep reporting suboptimal for an unchanged configuration, and
    /// reconfiguring every frame would stall presentation without converging.
    pub(crate) fn reconfigure_suboptimal(&mut self) {
        if !self.suboptimal_reconfigured {
            self.configure();
            self.suboptimal_reconfigured = true;
        }
    }
}

pub(crate) fn validate_size(
    graphics: &GraphicsContext,
    width: u32,
    height: u32,
) -> Result<(), Error> {
    let max = graphics.device().limits().max_texture_dimension_2d;
    if width > max || height > max {
        return Err(Error::InvalidTargetSize { width, height, max });
    }
    Ok(())
}

/// Selects a surface format and the sRGB format its views render to.
///
/// Prefers a natively sRGB format; otherwise, when surface view formats are
/// supported, a linear format with an sRGB view. `None` keeps the default.
fn select_formats(
    formats: &[wgpu::TextureFormat],
    view_formats: bool,
) -> Option<(wgpu::TextureFormat, wgpu::TextureFormat)> {
    if let Some(&format) = formats.iter().find(|format| format.is_srgb()) {
        return Some((format, format));
    }
    formats
        .iter()
        .map(|&format| (format, format.add_srgb_suffix()))
        .find(|(format, view)| view_formats && format != view)
}

/// Accepts automatic modes, which wgpu resolves, and modes the surface offers.
fn validate_presentation(
    capabilities: &wgpu::SurfaceCapabilities,
    present_mode: wgpu::PresentMode,
    alpha_mode: wgpu::CompositeAlphaMode,
) -> Result<(), Error> {
    use wgpu::{CompositeAlphaMode as A, PresentMode as P};
    let present = matches!(present_mode, P::AutoVsync | P::AutoNoVsync)
        || capabilities.present_modes.contains(&present_mode);
    let alpha = alpha_mode == A::Auto || capabilities.alpha_modes.contains(&alpha_mode);
    if present && alpha {
        Ok(())
    } else {
        Err(Error::UnsupportedPresentation {
            present_mode,
            alpha_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::TextureFormat::*;

    #[test]
    fn presentation_accepts_automatic_and_offered_modes_only() {
        use wgpu::{CompositeAlphaMode as A, PresentMode as P};
        let capabilities = wgpu::SurfaceCapabilities {
            formats: vec![Bgra8UnormSrgb],
            present_modes: vec![P::Fifo, P::Mailbox],
            alpha_modes: vec![A::Opaque, A::PreMultiplied],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format_capabilities: Vec::new(),
        };
        let defaults = SurfaceOptions::new(1, 1);
        assert_eq!(
            (defaults.present_mode, defaults.alpha_mode),
            (P::Fifo, A::Auto)
        );
        for (present, alpha) in [
            (P::Fifo, A::Auto),
            (P::Mailbox, A::PreMultiplied),
            (P::AutoNoVsync, A::Opaque),
            (P::AutoVsync, A::Auto),
        ] {
            let options = defaults.present_mode(present).alpha_mode(alpha);
            assert!(
                validate_presentation(&capabilities, options.present_mode, options.alpha_mode)
                    .is_ok()
            );
        }
        for (present, alpha) in [(P::Immediate, A::Auto), (P::Fifo, A::PostMultiplied)] {
            assert!(matches!(
                validate_presentation(&capabilities, present, alpha),
                Err(Error::UnsupportedPresentation { present_mode, alpha_mode })
                    if present_mode == present && alpha_mode == alpha
            ));
        }
        // A surface without FIFO still rejects the default.
        let no_fifo = wgpu::SurfaceCapabilities {
            present_modes: vec![P::Mailbox],
            ..capabilities
        };
        assert!(validate_presentation(&no_fifo, P::Fifo, A::Auto).is_err());
    }

    #[test]
    fn surface_formats_render_to_srgb_views() {
        assert_eq!(
            select_formats(&[Bgra8Unorm, Bgra8UnormSrgb], false),
            Some((Bgra8UnormSrgb, Bgra8UnormSrgb))
        );
        // WebGPU canvases offer only linear formats but allow sRGB views.
        assert_eq!(
            select_formats(&[Rgba16Float, Bgra8Unorm, Rgba8Unorm], true),
            Some((Bgra8Unorm, Bgra8UnormSrgb))
        );
        assert_eq!(select_formats(&[Bgra8Unorm], false), None);
        assert_eq!(select_formats(&[Rgba16Float], true), None);
    }
}
