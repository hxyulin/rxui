use crate::{MaterialOptions, TextureMaterialOptions};

/// Immutable blending, color writes, and depth/stencil state for built-in 2D shading.
///
/// Pass to a renderer's `with_options` constructor, or to
/// [`crate::Painter::with_options`] to configure all its built-in renderers.
/// Defaults use premultiplied source-over and disable depth/stencil tests and
/// writes. An explicit depth/stencil format must match the pass. Attachment
/// allocation, clipping, and dynamic stencil reference remain caller-owned.
///
/// Configuration is fixed for a renderer's lifetime. Keep multiple renderers for
/// different policies; their draws can share one pass. Preparing a variant caches
/// it by attachment formats and MSAA count. Changing the pass's stencil reference
/// neither creates a pipeline nor uploads geometry.
/// Built-in shading discards fully transparent fragments when depth/stencil writes
/// are enabled, so masks follow visible coverage instead of the enclosing quad.
/// Stencil masks are binary per sample. Analytic alpha is not a soft clip mask;
/// use a texture mask or multisampled mask geometry for fractional clip coverage.
#[derive(Clone, Debug)]
pub struct PipelineOptions {
    /// Blending for the built-in premultiplied fragment output. None replaces color.
    pub blend: Option<wgpu::BlendState>,
    /// Color channels written by the built-in shader.
    pub write_mask: wgpu::ColorWrites,
    /// Optional explicit tests/writes. The format must match the pass attachment.
    pub depth_stencil: Option<wgpu::DepthStencilState>,
}
impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
            depth_stencil: None,
        }
    }
}
impl PipelineOptions {
    /// Selects source-over shading with all color channels and no depth/stencil tests.
    pub fn new() -> Self {
        Self::default()
    }
    /// Selects blending; built-in shaders always output premultiplied color.
    pub fn blend(mut self, blend: Option<wgpu::BlendState>) -> Self {
        self.blend = blend;
        self
    }
    /// Selects the channels written; an empty mask supports stencil-only drawing.
    pub fn write_mask(mut self, mask: wgpu::ColorWrites) -> Self {
        self.write_mask = mask;
        self
    }
    /// Selects immutable tests/writes, or None to disable them.
    /// Dynamic references are set with [`crate::RenderPass::set_stencil_reference`].
    pub fn depth_stencil(mut self, state: Option<wgpu::DepthStencilState>) -> Self {
        self.depth_stencil = state;
        self
    }
    pub(crate) fn writes_attachment(&self) -> bool {
        self.depth_stencil
            .as_ref()
            .is_some_and(|state| !state.is_depth_read_only() || !state.is_stencil_read_only(None))
    }
    pub(crate) fn mesh<'a>(&self, mut options: MaterialOptions<'a>) -> MaterialOptions<'a> {
        options.blend = self.blend;
        options.write_mask = self.write_mask;
        options.depth_stencil = self.depth_stencil.clone();
        options
    }
    pub(crate) fn texture<'a>(
        &self,
        mut options: TextureMaterialOptions<'a>,
    ) -> TextureMaterialOptions<'a> {
        options.blend = self.blend;
        options.write_mask = self.write_mask;
        options.depth_stencil = self.depth_stencil.clone();
        options
    }
}
