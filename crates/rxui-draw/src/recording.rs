//! Command recording into an application-owned encoder.
use crate::{Error, Framebuffer, GraphicsContext, RenderPass, RenderPassDescriptor};
use crate::{frame::AttachmentWrites, uploads::DrawUploads};

/// Borrowed recording scope. Does not acquire a surface, submit, or present.
pub struct Recording<'a> {
    encoder: &'a mut wgpu::CommandEncoder,
    graphics: &'a GraphicsContext,
    uploads: DrawUploads,
    writes: AttachmentWrites,
}

impl<'a> Recording<'a> {
    /// Begins recording using an existing encoder on this device.
    pub fn new(graphics: &'a GraphicsContext, encoder: &'a mut wgpu::CommandEncoder) -> Self {
        Self {
            encoder,
            graphics,
            uploads: DrawUploads::new(&graphics.upload_pool),
            writes: Default::default(),
        }
    }

    /// Begins a pass over caller-owned attachments.
    pub fn begin_render_pass<'p>(
        &'p mut self,
        descriptor: &RenderPassDescriptor<'_>,
    ) -> Result<RenderPass<'p>, Error> {
        RenderPass::from_descriptor(
            self.encoder,
            self.graphics,
            &mut self.writes,
            &mut self.uploads,
            descriptor,
        )
    }

    /// Clears and renders an internal offscreen layer in the same recording.
    pub fn render_to<'p>(
        &'p mut self,
        target: &'p mut Framebuffer,
    ) -> crate::RenderPassBuilder<'p> {
        crate::RenderPassBuilder::for_framebuffer(
            self.encoder,
            self.graphics,
            target,
            &mut self.writes,
        )
        .with_uploads(&mut self.uploads)
    }

    /// Uploads recording-exclusive draw parameters, without submitting commands.
    /// Keep the returned resources alive until this encoder has been submitted or discarded.
    pub fn finish(self) -> Result<RecordingResources, Error> {
        self.writes.validate_submission()?;
        self.uploads.upload(self.graphics.queue());
        Ok(RecordingResources {
            uploads: self.uploads,
            writes: self.writes,
        })
    }
}

/// Resources pinned by commands in an external encoder.
///
/// After submitting that encoder, call `after_submit` on its queue. If discarding
/// the encoder, drop it before dropping this value. Do not release this value
/// while retaining unsubmitted commands: later preparation may recycle storage.
#[must_use = "retain until submission, then call after_submit; discard the encoder before dropping"]
pub struct RecordingResources {
    uploads: DrawUploads,
    writes: AttachmentWrites,
}
impl RecordingResources {
    /// Commits managed writes and retains allocations until queued work completes.
    /// Call immediately after submitting the owning encoder to this queue.
    pub fn after_submit(self, queue: &wgpu::Queue) {
        self.writes.commit();
        let uploads = self.uploads;
        queue.on_submitted_work_done(move || drop(uploads));
    }
}
