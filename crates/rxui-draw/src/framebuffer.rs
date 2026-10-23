use std::sync::{Arc, atomic::AtomicBool};

use crate::{Error, Frame, FrameError, GraphicsContext, pass::ManagedColorAttachment, target};

/// Initial configuration of a persistent offscreen color framebuffer.
///
/// Pass to [`GraphicsContext::create_framebuffer`]. Options default to a
/// single-sampled, linear RGBA8 texture usable for rendering and shader sampling.
/// Include [`wgpu::TextureUsages::COPY_SRC`] to enable GPU copies or readback.
/// Depth/stencil storage is disabled by default; enable it with [`Self::depth_stencil`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "pass these options to create_framebuffer"]
pub struct FramebufferOptions {
    /// Physical width and height. Either being zero suspends the resource.
    pub size: [u32; 2],
    /// Color format, validated against the device's enabled features.
    pub format: wgpu::TextureFormat,
    /// Color samples per pixel; one disables MSAA.
    pub sample_count: u32,
    /// Optional depth-only, stencil-only, or combined attachment format.
    pub depth_stencil_format: Option<wgpu::TextureFormat>,
    /// Usages of depth/stencil storage, independent of color usages.
    /// Must include rendering; sampling or copies can be enabled when supported.
    pub depth_stencil_usage: wgpu::TextureUsages,
    /// Usages of the single-sampled output. Must include `RENDER_ATTACHMENT`.
    pub usage: wgpu::TextureUsages,
}

impl FramebufferOptions {
    /// Selects a size with linear RGBA8, one sample, and rendering/sampling usages.
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            size: [width, height],
            format: wgpu::TextureFormat::Rgba8Unorm,
            sample_count: 1,
            depth_stencil_format: None,
            depth_stencil_usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                .union(wgpu::TextureUsages::TEXTURE_BINDING),
        }
    }

    /// Selects the color format. Validation occurs during creation.
    pub const fn format(mut self, format: wgpu::TextureFormat) -> Self {
        self.format = format;
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

    /// Selects the initial sample count. Unsupported counts have no implicit fallback.
    pub const fn sample_count(mut self, count: u32) -> Self {
        self.sample_count = count;
        self
    }

    /// Replaces output usages. Include rendering and any desired sampling/copy usages.
    ///
    /// Transient attachments are rejected because framebuffer contents persist.
    pub const fn usage(mut self, usage: wgpu::TextureUsages) -> Self {
        self.usage = usage;
        self
    }
}

#[derive(Debug)]
struct Attachments {
    view: wgpu::TextureView,
    texture: wgpu::Texture,
    multisample_view: Option<wgpu::TextureView>,
}

/// A device-bound, reusable offscreen color texture with optional MSAA.
///
/// Create through [`GraphicsContext::create_framebuffer`]. Render independently
/// with [`Self::begin_frame`], or in another frame with [`Frame::render_to`]. All
/// passes use the same renderer API as surfaces. The resolved output stays
/// single-sampled, even with MSAA enabled. Optional depth/stencil storage has
/// the same dimensions and sample count as the render attachment and is not resolved.
///
/// Contents persist after submission. The first managed pass must clear; later
/// load passes may preserve earlier drawing, including across recordings. Dropping
/// a frame submits nothing and does not initialize newly recorded attachments.
///
/// Resize and sample-count changes replace attachments and invalidate prior
/// contents. Rebuild application bind groups using the new [`Self::color_view`].
/// Make these changes between recordings. Raw handles cloned before a change
/// still reference the old textures. Zero dimensions release all attachments.
#[derive(Debug)]
pub struct Framebuffer {
    pub(crate) graphics: GraphicsContext,
    options: FramebufferOptions,
    attachments: Option<Attachments>,
    supported_sample_counts: Vec<u32>,
    pub(crate) initialized: Arc<AtomicBool>,
    pub(crate) samples_initialized: Arc<AtomicBool>,
    sampled: crate::SampledColor,
    pub(crate) depth_stencil: Option<crate::depth_stencil::DepthStencilAttachment>,
}

impl Framebuffer {
    pub(crate) fn create(
        graphics: &GraphicsContext,
        options: FramebufferOptions,
    ) -> Result<Self, Error> {
        target::validate_size(graphics, options.size[0], options.size[1])?;
        let format = options.format;
        let features = target::format_features(graphics, format);
        if format.is_depth_stencil_format()
            || format.is_multi_planar_format()
            || !graphics
                .device()
                .features()
                .contains(format.required_features())
            || !features
                .allowed_usages
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        {
            return Err(Error::UnsupportedColorFormat { format });
        }
        if !options
            .usage
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
            || options
                .usage
                .contains(wgpu::TextureUsages::TRANSIENT_ATTACHMENT)
            || !features.allowed_usages.contains(options.usage)
        {
            return Err(Error::InvalidFramebufferUsage {
                format,
                usage: options.usage,
            });
        }
        let mut supported_sample_counts = target::sample_counts(features);
        crate::depth_stencil::restrict_counts(
            graphics,
            &mut supported_sample_counts,
            options.depth_stencil_format,
            options.depth_stencil_usage,
        )?;
        target::validate_sample_count(format, options.sample_count, &supported_sample_counts)?;
        let attachments = allocate(graphics, options);
        let sampled =
            crate::SampledColor::new(graphics, attachments.as_ref().map(|a| a.view.clone()));
        Ok(Self {
            graphics: graphics.clone(),
            attachments,
            sampled,
            options,
            supported_sample_counts,
            initialized: Arc::new(AtomicBool::new(false)),
            samples_initialized: Arc::new(AtomicBool::new(false)),
            depth_stencil: crate::depth_stencil::allocate(
                graphics,
                options.size,
                options.sample_count,
                options.depth_stencil_format,
                options.depth_stencil_usage,
            ),
        })
    }

    /// Starts standalone command recording without acquiring a surface image.
    ///
    /// The frame exclusively borrows this resource until finish or drop. Finish
    /// submits once without presenting or waiting. A zero size returns
    /// [`FrameError::Suspended`]; there are no surface acquisition failures.
    pub fn begin_frame(&mut self) -> Result<Frame<'_, 'static>, FrameError> {
        self.begin_recording()
    }

    pub(crate) fn begin_recording<'window>(&mut self) -> Result<Frame<'_, 'window>, FrameError> {
        if self.options.size.contains(&0) {
            return Err(FrameError::Suspended);
        }
        Ok(Frame::for_framebuffer(self))
    }

    /// Replaces attachments at a changed physical size, discarding previous contents.
    ///
    /// A zero dimension releases attachments. Repeating the same size does nothing.
    /// Returns [`Error::InvalidTargetSize`] without changes if device limits are exceeded.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Error> {
        target::validate_size(&self.graphics, width, height)?;
        if self.options.size == [width, height] {
            return Ok(());
        }
        let options = FramebufferOptions {
            size: [width, height],
            ..self.options
        };
        self.replace(options);
        Ok(())
    }

    /// Changes MSAA using cached capabilities, replacing attachments and contents.
    ///
    /// A repeated count does nothing. Returns [`Error::UnsupportedSampleCount`]
    /// without changes for unsupported counts. No surface or capability query is made.
    pub fn set_sample_count(&mut self, count: u32) -> Result<(), Error> {
        if count == self.options.sample_count {
            return Ok(());
        }
        target::validate_sample_count(self.options.format, count, &self.supported_sample_counts)?;
        self.replace(FramebufferOptions {
            sample_count: count,
            ..self.options
        });
        Ok(())
    }

    /// Returns physical dimensions, including zero while suspended.
    pub fn size(&self) -> [u32; 2] {
        self.options.size
    }

    /// Returns the fixed color format.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.options.format
    }

    /// Returns the complete attachment format for pipeline preparation.
    pub fn render_format(&self) -> crate::RenderFormat {
        crate::RenderFormat {
            colors: vec![Some(self.format())],
            depth_stencil: self.depth_stencil_format(),
            sample_count: self.sample_count(),
        }
    }

    /// Returns the current color attachment sample count.
    pub fn sample_count(&self) -> u32 {
        self.options.sample_count
    }

    /// Borrows sample counts cached during creation without allocation or queries.
    pub fn supported_sample_counts(&self) -> &[u32] {
        &self.supported_sample_counts
    }

    /// Returns the usages configured for the single-sampled output.
    pub fn usage(&self) -> wgpu::TextureUsages {
        self.options.usage
    }

    /// Returns the optional depth/stencil format, including while suspended.
    pub fn depth_stencil_format(&self) -> Option<wgpu::TextureFormat> {
        self.options.depth_stencil_format
    }

    /// Returns depth/stencil usages, or `None` when this attachment is disabled.
    pub fn depth_stencil_usage(&self) -> Option<wgpu::TextureUsages> {
        self.options
            .depth_stencil_format
            .map(|_| self.options.depth_stencil_usage)
    }

    /// Borrows depth/stencil storage; disabled attachments return `None`.
    ///
    /// Returns `TargetSuspended` when enabled at zero size. Sampling/copies require
    /// matching creation usages and aspect-compatible views. Storage has the target's
    /// sample count and is never resolved. Raw writes do not mark managed contents
    /// initialized. Resize/MSAA replaces storage and invalidates old bindings.
    pub fn depth_stencil_texture(&self) -> Result<Option<&wgpu::Texture>, Error> {
        if self.options.depth_stencil_format.is_none() {
            return Ok(None);
        }
        Ok(Some(
            &self
                .depth_stencil
                .as_ref()
                .ok_or(Error::TargetSuspended)?
                .texture,
        ))
    }

    /// Borrows the attachment view, with the same rules as its texture.
    pub fn depth_stencil_view(&self) -> Result<Option<&wgpu::TextureView>, Error> {
        if self.options.depth_stencil_format.is_none() {
            return Ok(None);
        }
        Ok(Some(
            &self
                .depth_stencil
                .as_ref()
                .ok_or(Error::TargetSuspended)?
                .view,
        ))
    }

    /// Borrows the resolved single-sampled texture for copies and low-level integration.
    ///
    /// Returns [`Error::TargetSuspended`] at zero size. Raw writes do not update
    /// managed initialization; record a wrapped clear before using a managed load.
    pub fn color_texture(&self) -> Result<&wgpu::Texture, Error> {
        Ok(&self
            .attachments
            .as_ref()
            .ok_or(Error::TargetSuspended)?
            .texture)
    }

    /// Borrows the resolved output view for shader sampling and custom GPU work.
    ///
    /// Returns [`Error::TargetSuspended`] at zero size. Sampling requires
    /// `TEXTURE_BINDING` in the creation usages. Never sample an output while
    /// rendering into that same framebuffer in the same pass.
    pub fn color_view(&self) -> Result<&wgpu::TextureView, Error> {
        Ok(&self
            .attachments
            .as_ref()
            .ok_or(Error::TargetSuspended)?
            .view)
    }

    /// Copies the resolved color output to the CPU as tightly packed RGBA8 rows, top first.
    ///
    /// Submits a copy and blocks until the device finishes it (up to ten seconds), so use
    /// it for tests, screenshots and tools rather than every frame; it needs a native
    /// backend that can wait. Requires `COPY_SRC` usage and an `Rgba8`/`Bgra8` format,
    /// optionally sRGB; BGRA is swizzled to RGBA and sRGB bytes stay encoded. The output
    /// must hold submitted content, otherwise this returns `UninitializedFramebuffer`.
    pub fn read_rgba8(&mut self) -> Result<Vec<u8>, Error> {
        use wgpu::TextureFormat as F;
        let (format, usage) = (self.format(), self.usage());
        let bgra = match format {
            F::Rgba8Unorm | F::Rgba8UnormSrgb => false,
            F::Bgra8Unorm | F::Bgra8UnormSrgb => true,
            _ => return Err(Error::UnsupportedReadback { format, usage }),
        };
        if !usage.contains(wgpu::TextureUsages::COPY_SRC) {
            return Err(Error::UnsupportedReadback { format, usage });
        }
        let texture = self.color_texture()?;
        if !self.initialized.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::UninitializedFramebuffer);
        }
        let [width, height] = self.size();
        let row = width * 4;
        let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let device = self.graphics.device();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("RXUI drawing framebuffer readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        let submission = self.graphics.queue().submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| drop(tx.send(result)));
        let timeout = std::time::Duration::from_secs(10);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(timeout),
            })
            .map_err(|_| Error::ReadbackFailed)?;
        rx.recv_timeout(timeout)
            .map_err(|_| Error::ReadbackFailed)?
            .map_err(|_| Error::ReadbackFailed)?;
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|_| Error::ReadbackFailed)?;
        let mut pixels = Vec::with_capacity((row * height) as usize);
        for line in mapped.chunks(padded as usize) {
            pixels.extend_from_slice(&line[..row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        if bgra {
            for texel in pixels.as_chunks_mut::<4>().0 {
                texel.swap(0, 2);
            }
        }
        Ok(pixels)
    }

    /// Returns a live sampled-color handle that follows resize and MSAA changes.
    /// Its default alpha interpretation is premultiplied, as produced by built-in renderers.
    /// Custom shaders must declare a different interpretation when appropriate.
    pub fn sampled_color(&self) -> crate::SampledColor {
        self.sampled.clone()
    }

    /// Snapshots the current managed color attachment for a custom pass.
    /// Recorded descriptors retain this storage generation after a resize.
    pub fn color_attachment(&self) -> Result<crate::RenderColorAttachment, Error> {
        let a = self.attachment()?;
        let mut color = crate::RenderColorAttachment::new(a.view, a.format, a.size);
        color.resolve_target = a.resolve_target.cloned();
        color.state = Some(if self.sample_count() > 1 {
            self.samples_initialized.clone()
        } else {
            self.initialized.clone()
        });
        color.resolved_state = a.resolved_state.cloned();
        color.owner = Some(self.graphics.clone());
        Ok(color)
    }

    /// Snapshots managed depth/stencil storage for a custom pass, if enabled.
    pub fn depth_stencil_attachment(
        &self,
    ) -> Result<Option<crate::RenderDepthStencilAttachment>, Error> {
        if self.options.depth_stencil_format.is_none() {
            return Ok(None);
        }
        let a = self.depth_stencil.as_ref().ok_or(Error::TargetSuspended)?;
        Ok(Some(crate::RenderDepthStencilAttachment::managed(
            a,
            &self.graphics,
        )))
    }

    pub(crate) fn attachment(&self) -> Result<ManagedColorAttachment<'_>, Error> {
        let attachments = self.attachments.as_ref().ok_or(Error::TargetSuspended)?;
        Ok(ManagedColorAttachment {
            view: attachments
                .multisample_view
                .as_ref()
                .unwrap_or(&attachments.view),
            resolve_target: attachments
                .multisample_view
                .as_ref()
                .map(|_| &attachments.view),
            format: self.options.format,
            size: self.options.size,
            sample_count: self.options.sample_count,
            resolved_state: (self.options.sample_count > 1).then_some(&self.initialized),
        })
    }

    fn replace(&mut self, options: FramebufferOptions) {
        self.attachments = allocate(&self.graphics, options);
        self.depth_stencil = crate::depth_stencil::allocate(
            &self.graphics,
            options.size,
            options.sample_count,
            options.depth_stencil_format,
            options.depth_stencil_usage,
        );
        self.options = options;
        // Pending recordings retain the old generation's marker, never this one.
        self.initialized = Arc::new(AtomicBool::new(false));
        self.samples_initialized = Arc::new(AtomicBool::new(false));
        self.sampled
            .replace(self.attachments.as_ref().map(|a| a.view.clone()));
    }
}

impl From<Framebuffer> for crate::RenderTarget<'static> {
    fn from(framebuffer: Framebuffer) -> Self {
        Self::Framebuffer(framebuffer)
    }
}

fn allocate(graphics: &GraphicsContext, options: FramebufferOptions) -> Option<Attachments> {
    let [width, height] = options.size;
    if width == 0 || height == 0 {
        return None;
    }
    let texture = graphics.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("RXUI drawing framebuffer output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: options.format,
        usage: options.usage,
        view_formats: &[],
    });
    Some(Attachments {
        view: texture.create_view(&Default::default()),
        texture,
        multisample_view: target::create_multisample_view(
            graphics,
            options.format,
            options.format,
            options.size,
            options.sample_count,
        ),
    })
}

#[cfg(test)]
#[path = "framebuffer_tests.rs"]
pub(crate) mod tests;
