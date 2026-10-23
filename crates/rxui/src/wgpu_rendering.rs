//! Rendering into application-owned wgpu targets.
use crate::{Ui, UiError, UiPainter, View};
use rxui_draw::{RenderFormat, wgpu};

/// The destination for UI rendering. The application owns all attachments.
/// View format and mip dimensions must match the supplied view. The view must
/// support rendering. With MSAA, supply a matching single-sampled resolve view.
pub struct UiRenderTarget<'a> {
    /// Render attachment.
    pub view: &'a wgpu::TextureView,
    /// Optional MSAA resolve attachment.
    pub resolve_target: Option<&'a wgpu::TextureView>,
    /// Format of the render view.
    pub format: wgpu::TextureFormat,
    /// Physical dimensions of the selected mip.
    pub size: [u32; 2],
    /// Load operation. Use `Load` to overlay existing application content.
    pub load: wgpu::LoadOp<wgpu::Color>,
}
impl<'a> UiRenderTarget<'a> {
    /// Creates a destination which preserves existing application content.
    pub fn new(view: &'a wgpu::TextureView, format: wgpu::TextureFormat, size: [u32; 2]) -> Self {
        Self {
            view,
            resolve_target: None,
            format,
            size,
            load: wgpu::LoadOp::Load,
        }
    }
    /// Pipeline configuration for this attachment.
    pub fn render_format(&self) -> RenderFormat {
        RenderFormat::color(self.format, self.view.texture().sample_count())
    }
}

impl UiPainter {
    /// Records UI and intermediate opacity layers into an application-owned encoder.
    /// Call `Ui::prepare` and `UiPainter::prepare` first, using the destination's
    /// render format and the same scale. The caller submits and presents.
    ///
    /// Retain the returned resources until submission and call `after_submit` on
    /// the queue used to submit. If discarding, drop the encoder before resources.
    /// Multiple outstanding recordings retain independent parameter storage.
    pub fn record<T: View>(
        &mut self,
        ui: &Ui<T>,
        encoder: &mut wgpu::CommandEncoder,
        target: &UiRenderTarget<'_>,
        scale: f32,
    ) -> Result<rxui_draw::RecordingResources, UiError> {
        self.record_wgpu(ui, encoder, target, scale)
    }
}
