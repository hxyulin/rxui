use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    Error, Framebuffer, GraphicsContext, RenderPassBuilder, pass::ManagedColorAttachment,
    target::SurfaceTarget,
};

/// A reason a [`crate::RenderTarget`] could not provide a frame.
///
/// [`Self::Retry`] and [`Self::Suspended`] are expected acquisition outcomes.
/// Applications should schedule a later redraw for retry, or wait for a resize or
/// visibility change while suspended. Neither outcome means the device has failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FrameError {
    /// Acquisition timed out or remained outdated; schedule a later redraw.
    Retry,
    /// The target is zero-sized or occluded; wait for a resize or visibility change.
    Suspended,
    /// The surface was lost; recreate the target from the owning window.
    SurfaceLost,
    /// wgpu reported a surface acquisition validation failure.
    Validation,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Retry => "frame unavailable; schedule a later redraw",
            Self::Suspended => "frame acquisition suspended until resize or visibility changes",
            Self::SurfaceLost => "surface lost; recreate the surface and render target",
            Self::Validation => "wgpu rejected surface acquisition",
        })
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug, Default)]
pub(crate) struct AttachmentWrites {
    pending: Vec<(Arc<AtomicBool>, bool)>,
    required: Vec<(Arc<AtomicBool>, AttachmentAspect)>,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum AttachmentAspect {
    Color,
    Depth,
    Stencil,
    Surface,
}

impl AttachmentWrites {
    pub(crate) fn is_initialized(&self, state: &Arc<AtomicBool>) -> bool {
        self.pending
            .iter()
            .find(|(pending, _)| Arc::ptr_eq(pending, state))
            .map_or_else(
                || state.load(Ordering::Acquire),
                |(_, initialized)| *initialized,
            )
    }

    pub(crate) fn record(&mut self, state: &Arc<AtomicBool>, initialized: bool) {
        if let Some((_, pending)) = self
            .pending
            .iter_mut()
            .find(|(pending, _)| Arc::ptr_eq(pending, state))
        {
            *pending = initialized;
        } else {
            self.pending.push((Arc::clone(state), initialized));
        }
    }

    pub(crate) fn require(&mut self, state: &Arc<AtomicBool>, aspect: AttachmentAspect) {
        if !self.pending.iter().any(|(s, _)| Arc::ptr_eq(s, state))
            && !self.required.iter().any(|(s, _)| Arc::ptr_eq(s, state))
        {
            self.required.push((state.clone(), aspect));
        }
    }
    pub(crate) fn validate_submission(&self) -> Result<(), Error> {
        for (state, aspect) in &self.required {
            if !state.load(Ordering::Acquire) {
                return Err(match aspect {
                    AttachmentAspect::Color => Error::UninitializedFramebuffer,
                    AttachmentAspect::Depth => Error::UninitializedDepth,
                    AttachmentAspect::Stencil => Error::UninitializedStencil,
                    AttachmentAspect::Surface => Error::UninitializedFrame,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn commit(self) {
        for (state, initialized) in self.pending {
            state.store(initialized, Ordering::Release);
        }
    }
}

#[derive(Debug)]
enum FrameTarget<'target, 'window> {
    Surface {
        // Views precede the acquired image in drop order.
        view: wgpu::TextureView,
        image: wgpu::SurfaceTexture,
        target: &'target mut SurfaceTarget<'window>,
        suboptimal: bool,
    },
    Framebuffer(&'target mut Framebuffer),
}

impl FrameTarget<'_, '_> {
    fn graphics(&self) -> &GraphicsContext {
        match self {
            Self::Surface { target, .. } => &target.graphics,
            Self::Framebuffer(target) => &target.graphics,
        }
    }
}

/// Command recording for a window surface or an offscreen framebuffer.
///
/// Created by [`crate::RenderTarget::begin_frame`] or [`Framebuffer::begin_frame`].
/// The default destination is exclusively borrowed until finish or drop, preventing
/// its resize and another recording. Renderers are independent of this borrow.
/// [`Self::render_to`] records additional framebuffer passes into the same encoder.
///
/// [`Self::finish`] submits once and presents only for a surface frame. Dropping
/// the frame discards all recorded commands, releases any acquired image, and
/// does not commit framebuffer initialization. Submission never waits for GPU completion.
///
/// A destination cannot be resized while its frame is still in use:
///
/// ```compile_fail
/// use rxui_draw::{FrameError, RenderTarget};
/// fn resize_during_frame(target: &mut RenderTarget<'_>) -> Result<(), FrameError> {
///     let frame = target.begin_frame()?;
///     target.resize(640, 480).unwrap();
///     frame.finish().unwrap();
///     Ok(())
/// }
/// ```
///
/// End the pass before consuming its frame:
///
/// ```compile_fail
/// use rxui_draw::{Error, Frame};
/// fn finish_during_pass(mut frame: Frame<'_, '_>) -> Result<(), Error> {
///     let pass = frame.render_pass().begin()?;
///     frame.finish()?;
///     drop(pass);
///     Ok(())
/// }
/// ```
#[derive(Debug)]
#[must_use = "call finish to submit the frame; dropping it discards the recorded work"]
pub struct Frame<'target, 'window> {
    // Recording is released before an acquired image when abandoned.
    encoder: wgpu::CommandEncoder,
    target: FrameTarget<'target, 'window>,
    initialized: Arc<AtomicBool>,
    samples_initialized: Arc<AtomicBool>,
    uploads: crate::uploads::DrawUploads,
    writes: AttachmentWrites,
}

impl<'target, 'window> Frame<'target, 'window> {
    pub(crate) fn new(
        target: &'target mut SurfaceTarget<'window>,
        image: wgpu::SurfaceTexture,
        suboptimal: bool,
    ) -> Self {
        let view = image.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(target.format),
            ..Default::default()
        });
        let encoder = create_encoder(&target.graphics);
        let uploads = crate::uploads::DrawUploads::new(&target.graphics.upload_pool);
        Self {
            encoder,
            target: FrameTarget::Surface {
                view,
                image,
                target,
                suboptimal,
            },
            initialized: Arc::new(AtomicBool::new(false)),
            samples_initialized: Arc::new(AtomicBool::new(false)),
            uploads,
            writes: AttachmentWrites::default(),
        }
    }

    pub(crate) fn for_framebuffer(target: &'target mut Framebuffer) -> Self {
        let encoder = create_encoder(&target.graphics);
        let uploads = crate::uploads::DrawUploads::new(&target.graphics.upload_pool);
        Self {
            encoder,
            target: FrameTarget::Framebuffer(target),
            initialized: Arc::new(AtomicBool::new(false)),
            samples_initialized: Arc::new(AtomicBool::new(false)),
            uploads,
            writes: AttachmentWrites::default(),
        }
    }

    /// Configures a color pass writing to this frame's default destination.
    ///
    /// Defaults clear transparent black, store results, and use the full viewport
    /// and scissor. Later `load_color()` passes preserve individual MSAA samples.
    /// A surface frame's first pass must clear; a framebuffer may load contents
    /// from earlier submitted recordings. Dropping a builder records nothing.
    pub fn render_pass(&mut self) -> RenderPassBuilder<'_> {
        match &self.target {
            FrameTarget::Surface { view, target, .. } => RenderPassBuilder::new(
                &mut self.encoder,
                &target.graphics,
                ManagedColorAttachment {
                    view: target.multisample_view.as_ref().unwrap_or(view),
                    resolve_target: target.multisample_view.as_ref().map(|_| view),
                    format: target.format,
                    size: target.size,
                    sample_count: target.sample_count,
                    resolved_state: target.multisample_view.as_ref().map(|_| &self.initialized),
                },
                if target.sample_count > 1 {
                    &self.samples_initialized
                } else {
                    &self.initialized
                },
            )
            .with_depth_stencil(target.depth_stencil.as_ref(), &mut self.writes)
            .with_uploads(&mut self.uploads),
            FrameTarget::Framebuffer(target) => RenderPassBuilder::for_framebuffer(
                &mut self.encoder,
                &target.graphics,
                target,
                &mut self.writes,
            )
            .with_uploads(&mut self.uploads),
        }
    }

    /// Configures a pass writing to another framebuffer in this recording.
    ///
    /// Commands share the frame's encoder and submission. End this pass before
    /// sampling its resolved output in a subsequent pass. The destination is
    /// exclusively borrowed while its builder/pass is active. Change its size
    /// and MSAA between recordings; replacing attachments invalidates old views.
    ///
    /// `begin()` rejects another device, a zero-sized target, or an initial load
    /// with no submitted or earlier recorded clear. This pass never initializes
    /// a surface frame's default destination; clear that surface before finish.
    ///
    /// ```compile_fail
    /// use rxui_draw::{Error, Frame, Framebuffer};
    /// fn resize_during_pass(frame: &mut Frame<'_, '_>, target: &mut Framebuffer) -> Result<(), Error> {
    ///     let pass = frame.render_to(target).begin()?;
    ///     target.resize(640, 480)?;
    ///     drop(pass);
    ///     Ok(())
    /// }
    /// ```
    pub fn render_to<'pass>(
        &'pass mut self,
        target: &'pass mut Framebuffer,
    ) -> RenderPassBuilder<'pass> {
        RenderPassBuilder::for_framebuffer(
            &mut self.encoder,
            self.target.graphics(),
            target,
            &mut self.writes,
        )
        .with_uploads(&mut self.uploads)
    }

    /// Snapshots this frame's default color attachment, including its MSAA resolve.
    /// Use it in [`Self::begin_render_pass`] to customize a surface pass or add color outputs.
    pub fn color_attachment(&self) -> Result<crate::RenderColorAttachment, Error> {
        match &self.target {
            FrameTarget::Framebuffer(target) => target.color_attachment(),
            FrameTarget::Surface { view, target, .. } => {
                let mut a = crate::RenderColorAttachment::new(
                    target.multisample_view.as_ref().unwrap_or(view),
                    target.format,
                    target.size,
                );
                a.resolve_target = target.multisample_view.as_ref().map(|_| view.clone());
                a.state = Some(if target.sample_count > 1 {
                    self.samples_initialized.clone()
                } else {
                    self.initialized.clone()
                });
                a.resolved_state = target
                    .multisample_view
                    .as_ref()
                    .map(|_| self.initialized.clone());
                a.owner = Some(target.graphics.clone());
                Ok(a)
            }
        }
    }

    /// Snapshots the default target's optional depth/stencil attachment.
    pub fn depth_stencil_attachment(
        &self,
    ) -> Result<Option<crate::RenderDepthStencilAttachment>, Error> {
        match &self.target {
            FrameTarget::Framebuffer(target) => target.depth_stencil_attachment(),
            FrameTarget::Surface { target, .. } => Ok(target
                .depth_stencil
                .as_ref()
                .map(|a| crate::RenderDepthStencilAttachment::managed(a, &target.graphics))),
        }
    }

    /// Begins a custom pass with zero or multiple color outputs and imported or
    /// managed attachments. Managed operations participate in initialization tracking.
    /// Imported view formats and selected mip/layer sizes must be declared accurately;
    /// wgpu validates raw view compatibility. Submission still belongs to this frame.
    pub fn begin_render_pass<'pass>(
        &'pass mut self,
        descriptor: &crate::RenderPassDescriptor<'_>,
    ) -> Result<crate::RenderPass<'pass>, Error> {
        crate::RenderPass::from_descriptor(
            &mut self.encoder,
            self.target.graphics(),
            &mut self.writes,
            &mut self.uploads,
            descriptor,
        )
    }

    /// Records a custom full-color write to a single-sampled managed framebuffer.
    /// The callback receives its current output view and this recording's encoder.
    /// It must initialize every texel (for example with a copy or compute dispatch).
    /// Partial writes require already initialized contents. Prefer custom render passes
    /// when load/store operations describe the write. Panicking or dropping the frame
    /// does not commit initialization. MSAA sample storage is unaffected.
    pub fn write_framebuffer_color(
        &mut self,
        target: &mut Framebuffer,
        record: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
    ) -> Result<(), Error> {
        if !self.target.graphics().same_device(&target.graphics) {
            return Err(Error::DeviceMismatch);
        }
        record(&mut self.encoder, target.color_view()?);
        self.writes.record(&target.initialized, true);
        Ok(())
    }

    /// Borrows the command encoder for copies, compute, and custom GPU recording.
    ///
    /// Commands participate in `finish()` or are discarded when the frame drops.
    /// Use resources on the same device. Raw writes do not update framebuffer
    /// initialization, and a surface still needs a wrapped clear before finish.
    ///
    /// Encoder access is rejected while a pass is active:
    ///
    /// ```compile_fail
    /// use rxui_draw::{Error, Frame};
    /// fn encode_during_pass(mut frame: Frame<'_, '_>) -> Result<(), Error> {
    ///     let pass = frame.render_pass().begin()?;
    ///     frame.encoder().insert_debug_marker("cannot encode during a pass");
    ///     drop(pass);
    ///     Ok(())
    /// }
    /// ```
    pub fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
        &mut self.encoder
    }

    /// Returns the default destination's physical size, fixed for this recording.
    pub fn size(&self) -> [u32; 2] {
        match &self.target {
            FrameTarget::Surface { target, .. } => target.size,
            FrameTarget::Framebuffer(target) => target.size(),
        }
    }

    /// Returns the default destination's color format.
    pub fn format(&self) -> wgpu::TextureFormat {
        match &self.target {
            FrameTarget::Surface { target, .. } => target.format,
            FrameTarget::Framebuffer(target) => target.format(),
        }
    }

    /// Returns the default destination's optional depth/stencil format.
    pub fn depth_stencil_format(&self) -> Option<wgpu::TextureFormat> {
        match &self.target {
            FrameTarget::Surface { target, .. } => target.depth_stencil_format,
            FrameTarget::Framebuffer(target) => target.depth_stencil_format(),
        }
    }

    /// Returns the default destination's color sample count.
    ///
    /// Additional framebuffer passes can have different counts; custom renderers
    /// should use the active pass's count for pipeline selection.
    pub fn sample_count(&self) -> u32 {
        match &self.target {
            FrameTarget::Surface { target, .. } => target.sample_count,
            FrameTarget::Framebuffer(target) => target.sample_count(),
        }
    }

    /// Submits once, committing framebuffer writes and presenting a surface if owned.
    ///
    /// Offscreen frames submit without presenting. The submission index identifies
    /// queued work, not GPU completion. A suboptimal surface is reconfigured once after
    /// presentation, which may synchronize; it is not reconfigured again until its
    /// size changes or an optimal image is acquired. Empty offscreen recordings are allowed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UninitializedFrame`] without submitting when a surface output
    /// is uninitialized (including a missing MSAA resolve). Loads depending on earlier
    /// submissions can return color/depth/stencil initialization errors if an intervening
    /// recording discarded that storage. Clearing an additional framebuffer does not
    /// initialize the surface output.
    pub fn finish(mut self) -> Result<wgpu::SubmissionIndex, Error> {
        profiling::scope!("rxui_draw::Frame::finish");
        if matches!(self.target, FrameTarget::Surface { .. })
            && !self.writes.is_initialized(&self.initialized)
        {
            return Err(Error::UninitializedFrame);
        }
        let graphics = self.target.graphics().clone();
        // Keep tracked commits in the same order as managed queue submissions.
        let _submission_guard = graphics
            .submission_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.writes.validate_submission()?;
        self.uploads.upload(self.target.graphics().queue());
        let submission = self
            .target
            .graphics()
            .queue()
            .submit([self.encoder.finish()]);
        self.uploads.retain_until_complete(graphics.queue());
        self.writes.commit();
        if let FrameTarget::Surface {
            view,
            image,
            target,
            suboptimal,
        } = self.target
        {
            target.graphics.queue().present(image);
            drop(view);
            if suboptimal {
                target.reconfigure_suboptimal();
            }
        }
        Ok(submission)
    }
}

fn create_encoder(graphics: &GraphicsContext) -> wgpu::CommandEncoder {
    graphics
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("RXUI drawing frame"),
        })
}
