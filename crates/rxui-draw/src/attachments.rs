use crate::{GraphicsContext, depth_stencil::DepthStencilAttachment};
use std::sync::{Arc, atomic::AtomicBool};

/// Pipeline-compatible attachment formats and sample count, independent of a window.
/// Use this value to prepare pipelines before acquiring a frame.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderFormat {
    /// Color formats in shader output order; absent slots remain absent.
    pub colors: Vec<Option<wgpu::TextureFormat>>,
    /// Optional depth/stencil format.
    pub depth_stencil: Option<wgpu::TextureFormat>,
    /// Attachment sample count.
    pub sample_count: u32,
}
impl RenderFormat {
    pub(crate) fn single_color(&self) -> Result<wgpu::TextureFormat, crate::Error> {
        if self.colors.len() == 1 {
            self.colors[0].ok_or(crate::Error::ExpectedSingleColor)
        } else {
            Err(crate::Error::ExpectedSingleColor)
        }
    }

    /// Describes a single color output without depth/stencil.
    pub fn color(format: wgpu::TextureFormat, sample_count: u32) -> Self {
        Self {
            colors: vec![Some(format)],
            depth_stencil: None,
            sample_count,
        }
    }

    /// RGBA color format for an offscreen layer composited into a `target` color format.
    ///
    /// Keeps the target's encoding and precision so a layer round trip does not band or
    /// clip: sRGB targets use `Rgba8UnormSrgb`, 8-bit linear targets `Rgba8Unorm`, 32-bit
    /// float targets `Rgba32Float`, and every other target `Rgba16Float`. Layers are
    /// RGBA even for BGRA or narrower targets because they are sampled, not presented.
    /// `Rgba32Float` filters only with nearest sampling unless the device enables
    /// `FLOAT32_FILTERABLE`, and blends only with `FLOAT32_BLENDABLE`.
    pub fn layer_color(target: wgpu::TextureFormat) -> wgpu::TextureFormat {
        use wgpu::TextureFormat as F;
        match target {
            f if f.is_srgb() => F::Rgba8UnormSrgb,
            F::Rgba8Unorm | F::Bgra8Unorm => F::Rgba8Unorm,
            F::Rgba32Float | F::Rg32Float | F::R32Float => F::Rgba32Float,
            _ => F::Rgba16Float,
        }
    }

    /// The single-color format of an offscreen layer for this target: its color mapped
    /// by [`Self::layer_color`], the same sample count, and no depth/stencil.
    /// Fails with `ExpectedSingleColor` unless this format has exactly one color.
    pub fn layer(&self) -> Result<Self, crate::Error> {
        Ok(Self::color(
            Self::layer_color(self.single_color()?),
            self.sample_count,
        ))
    }
}

/// A snapshot of a color attachment and its operations for a custom pass.
/// Managed snapshots retain initialization tracking and the storage generation.
/// Imported views use wgpu initialization/validation; declare the view's format and
/// mip dimensions, which can differ from the underlying texture's metadata.
#[derive(Clone, Debug)]
pub struct RenderColorAttachment {
    /// Render view, including selected mip and layer.
    pub(crate) view: wgpu::TextureView,
    /// View format.
    pub(crate) format: wgpu::TextureFormat,
    /// Physical dimensions of the selected view.
    pub(crate) size: [u32; 2],
    /// Optional single-sampled resolve destination.
    pub(crate) resolve_target: Option<wgpu::TextureView>,
    resolve_enabled: bool,
    /// Color load and store operations; store applies to the render view.
    pub ops: wgpu::Operations<wgpu::Color>,
    /// Depth slice for a 3D color view.
    pub depth_slice: Option<u32>,
    pub(crate) state: Option<Arc<AtomicBool>>,
    pub(crate) resolved_state: Option<Arc<AtomicBool>>,
    pub(crate) owner: Option<GraphicsContext>,
}
impl RenderColorAttachment {
    /// Returns the retained view.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
    /// Returns the declared view format.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
    /// Returns the declared view dimensions.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }
    /// Imports a view with transparent clear and store defaults.
    pub fn new(view: &wgpu::TextureView, format: wgpu::TextureFormat, size: [u32; 2]) -> Self {
        Self {
            view: view.clone(),
            format,
            size,
            resolve_target: None,
            resolve_enabled: true,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
            state: None,
            resolved_state: None,
            owner: None,
        }
    }
    /// Replaces color load/store operations.
    pub fn ops(mut self, ops: wgpu::Operations<wgpu::Color>) -> Self {
        self.ops = ops;
        self
    }
    /// Loads and stores previous contents.
    pub fn load(mut self) -> Self {
        self.ops.load = wgpu::LoadOp::Load;
        self
    }
    /// Selects an application-provided resolve destination. Managed tracking of
    /// the original output is retained only when this is the original destination.
    pub fn resolve_to(mut self, view: &wgpu::TextureView) -> Self {
        if self.resolve_target.as_ref() != Some(view) {
            self.resolved_state = None;
        }
        self.resolve_target = Some(view.clone());
        self.resolve_enabled = true;
        self
    }
    /// Returns the configured resolve view, if present.
    pub fn resolve_target(&self) -> Option<&wgpu::TextureView> {
        if self.resolve_enabled {
            self.resolve_target.as_ref()
        } else {
            None
        }
    }
    /// Enables or omits resolving to the snapshot's configured output.
    pub fn resolve(mut self, enabled: bool) -> Self {
        self.resolve_enabled = enabled;
        self
    }
}

/// A snapshot of an imported or managed depth/stencil attachment.
#[derive(Clone, Debug)]
pub struct RenderDepthStencilAttachment {
    /// Depth/stencil view.
    pub(crate) view: wgpu::TextureView,
    /// View format.
    pub(crate) format: wgpu::TextureFormat,
    /// Physical dimensions of the selected view.
    pub(crate) size: [u32; 2],
    /// Depth operations, or `None` for read-only access.
    pub depth_ops: Option<wgpu::Operations<f32>>,
    /// Stencil operations, or `None` for read-only access.
    pub stencil_ops: Option<wgpu::Operations<u32>>,
    pub(crate) depth_state: Option<Arc<AtomicBool>>,
    pub(crate) stencil_state: Option<Arc<AtomicBool>>,
    pub(crate) owner: Option<GraphicsContext>,
}
impl RenderDepthStencilAttachment {
    /// Returns the retained view.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
    /// Returns the declared view format.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
    /// Returns the declared view dimensions.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }
    /// Imports a view, clearing and storing its available aspects.
    pub fn new(view: &wgpu::TextureView, format: wgpu::TextureFormat, size: [u32; 2]) -> Self {
        Self {
            view: view.clone(),
            format,
            size,
            depth_ops: format.has_depth_aspect().then_some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: format.has_stencil_aspect().then_some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0),
                store: wgpu::StoreOp::Store,
            }),
            depth_state: None,
            stencil_state: None,
            owner: None,
        }
    }
    /// Loads and stores each available aspect.
    pub fn load(mut self) -> Self {
        if let Some(ops) = &mut self.depth_ops {
            ops.load = wgpu::LoadOp::Load;
        }
        if let Some(ops) = &mut self.stencil_ops {
            ops.load = wgpu::LoadOp::Load;
        }
        self
    }
    pub(crate) fn managed(a: &DepthStencilAttachment, g: &GraphicsContext) -> Self {
        let mut result = Self::new(
            &a.view,
            a.texture.format(),
            [a.texture.width(), a.texture.height()],
        );
        result.depth_state = Some(a.depth_initialized.clone());
        result.stencil_state = Some(a.stencil_initialized.clone());
        result.owner = Some(g.clone());
        result
    }
}

/// Explicit attachments for a custom pass. Supports depth-only passes and multiple
/// color outputs. Default viewport/scissor cover the first available attachment.
/// Imported views may select arbitrary mips/layers. wgpu validates raw handles,
/// usages, resolve compatibility, and optional query features.
#[derive(Debug, Default)]
pub struct RenderPassDescriptor<'a> {
    /// Debug label.
    pub label: Option<&'a str>,
    /// Color slots in shader output order.
    pub colors: &'a [Option<RenderColorAttachment>],
    /// Optional depth/stencil attachment.
    pub depth_stencil: Option<&'a RenderDepthStencilAttachment>,
    /// Optional initial viewport in pixels with minimum/maximum depth.
    pub viewport: Option<[f32; 6]>,
    /// Optional initial scissor in pixels.
    pub scissor: Option<[u32; 4]>,
    /// Dynamic stencil comparison/replacement reference.
    pub stencil_reference: u32,
    /// Optional timestamp writes; requires enabled wgpu timestamp features.
    pub timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'a>>,
    /// Optional occlusion query set.
    pub occlusion_query_set: Option<&'a wgpu::QuerySet>,
}
