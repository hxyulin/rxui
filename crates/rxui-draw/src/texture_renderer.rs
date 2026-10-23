use crate::{Error, GraphicsContext, Rect, RenderPass, RenderTarget};
use bytemuck::{Pod, Zeroable};
use std::{
    borrow::Cow,
    collections::{HashMap, hash_map::Entry},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// Alpha encoding of sampled RGB, independent of destination blending.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureAlpha {
    /// RGB is independent of alpha, as in most uploaded images.
    ///
    /// Texels are premultiplied before filtering, so the RGB of transparent texels
    /// never darkens edges. Linear filtering interpolates the view's first mip level
    /// with the sampler's address modes; use premultiplied data for mipmapped or
    /// anisotropic sampling. Where neighbouring texels differ in alpha, this
    /// filtering fetches four texels instead of one; premultiplied data is the
    /// cheaper choice for large, layered or alpha-gradient images.
    #[default]
    Straight,
    /// RGB already contains alpha, as in built-in framebuffer rendering.
    Premultiplied,
}
/// Convenience sampling filter. Custom samplers can be supplied separately.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureFilter {
    /// Nearest texel; supports unfilterable float formats.
    Nearest,
    /// Linear interpolation; requires filterable storage.
    #[default]
    Linear,
}
/// Destination blending for built-in texture shading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureBlend {
    /// Premultiplied source-over blending.
    #[default]
    Alpha,
    /// Replace destination color with premultiplied output.
    Replace,
}
impl From<TextureBlend> for Option<wgpu::BlendState> {
    fn from(blend: TextureBlend) -> Self {
        match blend {
            TextureBlend::Alpha => Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            TextureBlend::Replace => None,
        }
    }
}

/// A normalized source rectangle in texture UV coordinates, top-left based.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvRect {
    /// Left UV coordinate.
    pub u: f32,
    /// Top UV coordinate.
    pub v: f32,
    /// Nonnegative UV width.
    pub width: f32,
    /// Nonnegative UV height.
    pub height: f32,
}
impl UvRect {
    /// Creates a UV rectangle. Coordinates outside the texture follow sampler addressing.
    pub const fn new(u: f32, v: f32, width: f32, height: f32) -> Self {
        Self {
            u,
            v,
            width,
            height,
        }
    }
    fn array(self) -> [f32; 4] {
        [self.u, self.v, self.width, self.height]
    }
}
/// Per-draw CPU data. Changing these values allocates no binding, sampler, or uniform buffer.
/// Parameters are copied into recording-owned instance storage and uploaded at finish.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureDraw {
    destination: Rect,
    normalized: bool,
    /// Source UV rectangle.
    pub uv: UvRect,
    /// Linear RGB tint and straight opacity, with opacity in `0..=1`.
    pub tint: [f32; 4],
    /// Affine transform `[xx, yx, xy, yy, tx, ty]` in destination units.
    /// Applied before conversion from pixels or normalized viewport coordinates.
    pub transform: crate::Transform2D,
}
impl Default for TextureDraw {
    fn default() -> Self {
        Self::normalized(Rect::new(0., 0., 1., 1.))
    }
}
impl TextureDraw {
    /// Selects a rectangle in physical pixels relative to the viewport's top-left.
    pub const fn new(destination: Rect) -> Self {
        Self {
            destination,
            normalized: false,
            uv: UvRect::new(0., 0., 1., 1.),
            tint: [1.; 4],
            transform: crate::Transform2D::IDENTITY,
        }
    }
    /// Selects a viewport-relative rectangle, where `(1,1)` is its bottom-right.
    pub const fn normalized(destination: Rect) -> Self {
        Self {
            normalized: true,
            ..Self::new(destination)
        }
    }
    /// Selects pixel or normalized units for placement and its affine transform.
    pub const fn space(mut self, space: crate::DrawSpace) -> Self {
        self.normalized = matches!(space, crate::DrawSpace::Normalized);
        self
    }
    /// Selects normalized source UVs.
    pub const fn uv(mut self, uv: UvRect) -> Self {
        self.uv = uv;
        self
    }
    /// Selects tint and opacity.
    pub const fn tint(mut self, tint: [f32; 4]) -> Self {
        self.tint = tint;
        self
    }
    /// Selects an affine destination transform.
    pub fn transform(mut self, transform: impl Into<crate::Transform2D>) -> Self {
        self.transform = transform.into();
        self
    }
    fn parameters(self, viewport: [f32; 2]) -> Result<Parameters, Error> {
        let valid = |r: [f32; 4]| r.iter().all(|v| v.is_finite()) && r[2] >= 0. && r[3] >= 0.;
        if !valid(self.destination.array())
            || !valid(self.uv.array())
            || !self
                .tint
                .iter()
                .chain(&self.transform.0)
                .all(|v| v.is_finite())
            || !(0.0..=1.0).contains(&self.tint[3])
        {
            return Err(Error::InvalidTextureDraw);
        }
        let [xx, yx, xy, yy, tx, ty] = self.transform.0;
        let scale = if self.normalized {
            [1., 1.]
        } else {
            viewport.map(|v| if v > 0. { 1. / v } else { 0. })
        };
        let r = self.destination;
        let p = Parameters {
            origin_axis_x: [
                (xx * r.x + xy * r.y + tx) * scale[0],
                (yx * r.x + yy * r.y + ty) * scale[1],
                xx * r.width * scale[0],
                yx * r.width * scale[1],
            ],
            axis_y: [
                xy * r.height * scale[0],
                yy * r.height * scale[1],
                f32::from(self.normalized),
                0.,
            ],
            source: self.uv.array(),
            tint: self.tint,
        };
        if !bytemuck::cast_slice::<Parameters, f32>(&[p])
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(Error::InvalidTextureDraw);
        }
        Ok(p)
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Parameters {
    origin_axis_x: [f32; 4],
    axis_y: [f32; 4],
    source: [f32; 4],
    tint: [f32; 4],
}
/// One image/material bound to a pass, with reusable scratch storage for dynamic draws.
///
/// Created by [`TextureRenderer::bind`]. Changing rectangles retain data validation;
/// fixed image compatibility and pipeline selection are amortized over the scope.
/// A managed source is a snapshot for the scope's duration. [`Self::pass`] supports
/// clipping, application bindings, and other renderers with automatic restoration
/// of this scope's pipeline/image before its next draw.
#[derive(Debug)]
pub struct TextureDrawSession<'draw, 'frame> {
    pass: &'draw mut RenderPass<'frame>,
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    parameters: &'draw mut Vec<Parameters>,
    dirty: bool,
    builtin_clip: bool,
}
impl<'draw, 'frame> TextureDrawSession<'draw, 'frame> {
    /// Validates and records one changing rectangle through reusable upload pages.
    pub fn draw(&mut self, draw: TextureDraw) -> Result<(), Error> {
        let parameters = draw.parameters(self.pass.viewport_size())?;
        self.restore();
        record_parameters(
            self.pass,
            std::slice::from_ref(&parameters),
            self.builtin_clip,
        );
        Ok(())
    }
    /// Validates an entire slice before recording instanced batches in input order.
    /// Large slices split at the same upload-page boundaries as [`TextureRenderer::draw_many`].
    pub fn draw_many(&mut self, draws: &[TextureDraw]) -> Result<(), Error> {
        self.parameters.clear();
        let viewport = self.pass.viewport_size();
        for draw in draws {
            self.parameters.push(draw.parameters(viewport)?);
        }
        if self.parameters.is_empty() {
            return Ok(());
        }
        self.restore();
        record_parameters(self.pass, self.parameters, self.builtin_clip);
        Ok(())
    }
    /// Binds immutable instance data for repeated draws without uploads or per-draw resource checks.
    /// Pixel-space data must match the current viewport. Normalized data adapts to it.
    /// The child scope borrows this image scope; dropping it returns control here.
    pub fn bind_prepared<'prepared>(
        &'prepared mut self,
        draws: &'prepared PreparedTextureDraw,
    ) -> Result<PreparedTextureDrawSession<'prepared, 'frame>, Error> {
        if !self.pass.same_device(&draws.graphics) {
            return Err(Error::DeviceMismatch);
        }
        validate_prepared_viewport(self.pass, draws)?;
        self.restore();
        self.pass
            .set_vertex_buffer(0, &draws.buffer, 0..draws.buffer.size());
        // Child pass access can change resources/raster state, so restore on return.
        self.dirty = true;
        Ok(PreparedTextureDrawSession {
            pass: self.pass,
            pipeline: self.pipeline.clone(),
            group: self.group.clone(),
            builtin_clip: self.builtin_clip,
            draws,
            dirty: false,
        })
    }
    /// Borrows the pass for clipping, bindings, raw access, or another renderer.
    /// The next scoped draw restores the bound pipeline/image and wrapped raster state.
    pub fn pass(&mut self) -> &mut RenderPass<'frame> {
        self.dirty = true;
        self.pass
    }
    fn restore(&mut self) {
        if self.dirty {
            self.pass.apply_raster_state();
            self.pass.set_pipeline(&self.pipeline);
            self.pass.set_bind_group(0, &self.group, &[]);
            self.dirty = false;
        }
    }
}

/// An image/material and immutable instance data bound for repeated draws.
///
/// Prepared binding validates the device and viewport once. Repeated draws emit
/// commands directly. Accessing the pass requires viewport revalidation and state
/// restoration before the next draw. Obtain directly with [`TextureRenderer::bind_prepared`]
/// or borrow a child scope through [`TextureDrawSession::bind_prepared`].
#[derive(Debug)]
pub struct PreparedTextureDrawSession<'draw, 'frame> {
    pass: &'draw mut RenderPass<'frame>,
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    draws: &'draw PreparedTextureDraw,
    dirty: bool,
    builtin_clip: bool,
}
impl<'draw, 'frame> PreparedTextureDrawSession<'draw, 'frame> {
    /// Records the bound instances without recurring uploads or pipeline lookup.
    /// Returns [`Error::InvalidTextureDraw`] if pass access changed a pixel-space
    /// viewport incompatibly; no draw is recorded and the scope remains usable.
    #[inline]
    pub fn draw(&mut self) -> Result<(), Error> {
        if self.dirty {
            validate_prepared_viewport(self.pass, self.draws)?;
            self.pass.apply_raster_state();
            self.pass.set_pipeline(&self.pipeline);
            self.pass.set_bind_group(0, &self.group, &[]);
            self.pass
                .set_vertex_buffer(0, &self.draws.buffer, 0..self.draws.buffer.size());
            self.dirty = false;
        }
        self.pass.bind_clip(2);
        if self.builtin_clip {
            self.pass.bind_rounded_clips();
        }
        self.pass.draw(0..6, 0..self.draws.count);
        Ok(())
    }
    /// Borrows the pass, invalidating binding assumptions until the next draw.
    /// Clipping and other renderer/raw operations remain available. Pixel-space
    /// prepared data is checked again if the viewport changes.
    pub fn pass(&mut self) -> &mut RenderPass<'frame> {
        self.dirty = true;
        self.pass
    }
}
#[inline]
fn validate_prepared_viewport(
    pass: &RenderPass<'_>,
    draws: &PreparedTextureDraw,
) -> Result<(), Error> {
    if draws.viewport.is_some_and(|v| v != pass.viewport_size()) {
        return Err(Error::InvalidTextureDraw);
    }
    Ok(())
}
fn record_parameters(pass: &mut RenderPass<'_>, parameters: &[Parameters], builtin_clip: bool) {
    if builtin_clip {
        pass.bind_rounded_clips();
    }
    pass.bind_clip(2);
    for chunk in parameters.chunks(1024) {
        let first = pass.bind_instances(0, bytemuck::cast_slice(chunk), 64);
        pass.draw_instances(
            crate::pass::DrawGeometry::Vertices(0..6),
            first..first + chunk.len() as u32,
        );
    }
}

fn parameter_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4];
    wgpu::VertexBufferLayout {
        array_stride: 64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}
/// Reusable sampling settings, independent of rectangle placement and material state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureBindingOptions {
    /// Source RGB alpha encoding.
    pub alpha: TextureAlpha,
    /// Filtering requirement; must match a supplied custom sampler.
    pub filter: TextureFilter,
}
impl TextureBindingOptions {
    /// Straight alpha and linear sampling.
    pub const fn new() -> Self {
        Self {
            alpha: TextureAlpha::Straight,
            filter: TextureFilter::Linear,
        }
    }
    /// Selects source alpha interpretation.
    pub const fn alpha(mut self, alpha: TextureAlpha) -> Self {
        self.alpha = alpha;
        self
    }
    /// Selects sampling filtering requirements.
    pub const fn filter(mut self, filter: TextureFilter) -> Self {
        self.filter = filter;
        self
    }
}
/// Live framebuffer output reference. Clones follow replacement of its storage.
/// Snapshot texture bindings remain available for explicit generation ownership.
#[derive(Clone, Debug)]
pub struct SampledColor {
    view: Arc<RwLock<Option<wgpu::TextureView>>>,
    graphics: GraphicsContext,
}
impl SampledColor {
    pub(crate) fn new(graphics: &GraphicsContext, view: Option<wgpu::TextureView>) -> Self {
        Self {
            view: Arc::new(RwLock::new(view)),
            graphics: graphics.clone(),
        }
    }
    pub(crate) fn replace(&self, view: Option<wgpu::TextureView>) {
        *self.view.write().unwrap_or_else(|e| e.into_inner()) = view;
    }
    /// Snapshots the current view; suspended output has no storage.
    pub fn view(&self) -> Result<wgpu::TextureView, Error> {
        self.view
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(Error::TargetSuspended)
    }
}
/// Immutable texture instance data uploaded once, independently of image bindings.
/// Use for static content; changing draws use [`TextureDraw`] and recording-owned uploads.
/// Pixel-coordinate data is prepared against a viewport size and rejects a different
/// viewport size at draw time. Normalized data adapts to any viewport without reupload.
#[derive(Clone, Debug)]
pub struct PreparedTextureDraw {
    graphics: GraphicsContext,
    buffer: wgpu::Buffer,
    count: u32,
    viewport: Option<[f32; 2]>,
    bounds: [Option<[f64; 4]>; 2],
}

type LiveBinding = (
    SampledColor,
    Arc<Mutex<(wgpu::TextureView, wgpu::BindGroup)>>,
);

/// A reusable image/sampler binding. Draw placement, UVs, tint, and transforms
/// are supplied separately. Clones share handles; recorded draws retain their groups.
#[derive(Clone, Debug)]
pub struct TextureBinding {
    graphics: GraphicsContext,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    group: wgpu::BindGroup,
    options: TextureBindingOptions,
    managed: Option<LiveBinding>,
}
impl TextureBinding {
    /// Returns source alpha and filtering settings.
    pub fn options(&self) -> TextureBindingOptions {
        self.options
    }
    /// Returns the snapshot used at creation or explicit rebind.
    /// For live framebuffer sources use [`SampledColor::view`] to inspect current storage.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}
/// Immutable shader and pipeline settings for textured rectangles.
#[derive(Clone, Debug)]
pub struct TextureMaterialOptions<'a> {
    /// Blend operation for built-in premultiplied output.
    pub blend: Option<wgpu::BlendState>,
    /// Color channels written by the material.
    pub write_mask: wgpu::ColorWrites,
    /// Winding used for culling and front/back stencil operations.
    pub front_face: wgpu::FrontFace,
    /// Faces to discard; defaults to no culling.
    pub cull_mode: Option<wgpu::Face>,
    /// Optional explicit depth/stencil tests and writes.
    pub depth_stencil: Option<wgpu::DepthStencilState>,
    /// Optional custom shader. Uses the four vec4 instance attributes documented
    /// by [`TextureDraw`], and image/sampler at group zero bindings zero/one.
    /// Locations seven to nine carry only the innermost [`crate::RoundedClip`].
    /// For nested intersections, prepend [`GraphicsContext::rounded_clip_shader`], declare
    /// the context's rounded clip layout at group one, multiply premultiplied output
    /// by `rounded_clip_coverage(fragment_position.xy)`, and call
    /// [`RenderPass::bind_rounded_clips`] before drawing. Without explicit shader
    /// integration only the scissor clips custom output.
    pub shader: Option<&'a wgpu::ShaderModule>,
    /// Vertex entry point for a custom shader.
    pub vertex_entry: &'a str,
    /// Custom fragment entry point. Built-in shading selects source alpha automatically.
    pub fragment_entry: &'a str,
    /// Additional layouts at groups one and above, bound through [`RenderPass::set_bind_group`].
    pub bind_group_layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
}
impl Default for TextureMaterialOptions<'_> {
    fn default() -> Self {
        Self::new()
    }
}
impl<'a> TextureMaterialOptions<'a> {
    /// Built-in shading with alpha blending and no depth/stencil tests or writes.
    pub const fn new() -> Self {
        Self {
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            depth_stencil: None,
            shader: None,
            vertex_entry: "vertex_main",
            fragment_entry: "fragment_main",
            bind_group_layouts: &[],
        }
    }
    /// Selects blending.
    pub fn blend(mut self, blend: impl Into<Option<wgpu::BlendState>>) -> Self {
        self.blend = blend.into();
        self
    }
    /// Selects depth/stencil state. Dynamic stencil reference belongs to the pass.
    pub fn depth_stencil(mut self, state: Option<wgpu::DepthStencilState>) -> Self {
        self.depth_stencil = state;
        self
    }
}
/// Reusable texture shading settings, independent of targets and image bindings.
/// Clones share a cache identity and lifetime. After the last clone is dropped,
/// [`TextureRenderer::prune_pipeline_cache`] can reclaim its cached variants.
#[derive(Clone, Debug)]
pub struct TextureMaterial {
    id: u64,
    lifetime: Arc<()>,
    graphics: GraphicsContext,
    blend: Option<wgpu::BlendState>,
    write_mask: wgpu::ColorWrites,
    front_face: wgpu::FrontFace,
    cull_mode: Option<wgpu::Face>,
    depth_stencil: Option<wgpu::DepthStencilState>,
    shader: Option<wgpu::ShaderModule>,
    vertex_entry: String,
    fragment_entry: String,
    layouts: Vec<Option<wgpu::BindGroupLayout>>,
}
impl TextureMaterial {
    pub(crate) fn create(g: &GraphicsContext, o: TextureMaterialOptions<'_>) -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        Self {
            id: ID.fetch_add(1, Ordering::Relaxed),
            lifetime: Arc::new(()),
            graphics: g.clone(),
            blend: o.blend,
            write_mask: o.write_mask,
            front_face: o.front_face,
            cull_mode: o.cull_mode,
            depth_stencil: o.depth_stencil,
            shader: o.shader.cloned(),
            vertex_entry: o.vertex_entry.into(),
            fragment_entry: o.fragment_entry.into(),
            layouts: o.bind_group_layouts.iter().map(|v| v.cloned()).collect(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PipelineKey {
    material: u64,
    format: wgpu::TextureFormat,
    count: u32,
    depth_stencil: Option<wgpu::TextureFormat>,
    filter: TextureFilter,
    alpha: TextureAlpha,
    transformed: bool,
}
/// Independent textured-rectangle renderer. Records explicit draw order into
/// application-owned passes. Instance pages are leased per recording and uploaded
/// once per page at finish; image bindings are reusable across changing draws.
/// Use [`Self::prune_pipeline_cache`] after material churn, or
/// [`Self::clear_pipeline_cache`] to release all variants. Draws never scan for
/// dropped materials.
#[derive(Debug)]
pub struct TextureRenderer {
    graphics: GraphicsContext,
    shader: wgpu::ShaderModule,
    nearest: wgpu::BindGroupLayout,
    linear: wgpu::BindGroupLayout,
    nearest_sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,
    material: Arc<TextureMaterial>,
    pipelines: HashMap<PipelineKey, crate::pipeline_cache::CachedPipeline>,
    parameters: Vec<Parameters>,
}
impl TextureRenderer {
    /// Counts cached variants of live and dropped materials. Scans the cache.
    pub fn pipeline_cache_stats(&self) -> crate::PipelineCacheStats {
        crate::pipeline_cache::stats(&self.pipelines)
    }

    /// Releases cached variants whose material and all its clones were dropped.
    ///
    /// Returns the number removed and releases unused cache storage. Call at an
    /// application-chosen maintenance point to bound material churn. This scans
    /// the cache; draws never scan it. Live materials retain their prepared variants.
    /// Held draw scopes and recorded commands retain independent pipeline handles
    /// and remain valid. GPU destruction may wait for those handles and GPU work.
    pub fn prune_pipeline_cache(&mut self) -> usize {
        crate::pipeline_cache::prune(&mut self.pipelines)
    }

    /// Releases all cache handles and storage, returning the number removed.
    ///
    /// Held scopes and recorded commands remain valid. Future draws or preparation
    /// recreate evicted variants, including variants of the default material.
    pub fn clear_pipeline_cache(&mut self) -> usize {
        let removed = self.pipelines.len();
        self.pipelines = HashMap::new();
        removed
    }

    /// Creates built-in shader, layouts, reusable samplers, and an empty pipeline cache.
    pub fn new(g: &GraphicsContext) -> Self {
        Self::with_options(g, crate::PipelineOptions::default())
    }
    /// Creates built-in image shading with immutable blend/write/depth/stencil settings.
    /// Source bindings remain reusable; custom materials can override this policy.
    pub fn with_options(g: &GraphicsContext, options: crate::PipelineOptions) -> Self {
        let device = g.device();
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        Self {
            graphics: g.clone(),
            shader: device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("RXUI drawing textures"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::clip::shader(g, include_str!("texture.wgsl")).into(),
                ),
            }),
            nearest: binding_layout(device, false),
            linear: binding_layout(device, true),
            nearest_sampler: sampler(wgpu::FilterMode::Nearest),
            linear_sampler: sampler(wgpu::FilterMode::Linear),
            material: Arc::new(
                g.create_texture_material(options.texture(TextureMaterialOptions::new())),
            ),
            pipelines: HashMap::new(),
            parameters: Vec::new(),
        }
    }
    fn layout(&self, filter: TextureFilter) -> &wgpu::BindGroupLayout {
        match filter {
            TextureFilter::Nearest => &self.nearest,
            TextureFilter::Linear => &self.linear,
        }
    }
    /// Creates a snapshot image binding using a reusable nearest or linear sampler.
    pub fn create_binding(
        &self,
        view: &wgpu::TextureView,
        options: TextureBindingOptions,
    ) -> Result<TextureBinding, Error> {
        self.create_binding_with_sampler(
            view,
            match options.filter {
                TextureFilter::Nearest => &self.nearest_sampler,
                TextureFilter::Linear => &self.linear_sampler,
            },
            options,
        )
    }
    /// Uses an application sampler for addressing, filtering, and mip selection.
    /// Its filtering behavior must match `options.filter`; wgpu validates the raw sampler.
    pub fn create_binding_with_sampler(
        &self,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        options: TextureBindingOptions,
    ) -> Result<TextureBinding, Error> {
        self.validate_source(view, options.filter)?;
        let group = bind(
            self.graphics.device(),
            self.layout(options.filter),
            view,
            sampler,
        );
        Ok(TextureBinding {
            graphics: self.graphics.clone(),
            view: view.clone(),
            sampler: sampler.clone(),
            group,
            options,
            managed: None,
        })
    }
    /// Binds a live framebuffer source with premultiplied alpha by default.
    /// The source refreshes only when storage changes; a suspended source rejects drawing.
    pub fn create_sampled_binding(&self, source: &SampledColor) -> Result<TextureBinding, Error> {
        self.create_sampled_binding_with_options(
            source,
            TextureBindingOptions::new().alpha(TextureAlpha::Premultiplied),
        )
    }
    /// Selects explicit alpha/filtering for a live source, including custom shader output.
    pub fn create_sampled_binding_with_options(
        &self,
        source: &SampledColor,
        options: TextureBindingOptions,
    ) -> Result<TextureBinding, Error> {
        if !self.graphics.same_device(&source.graphics) {
            return Err(Error::DeviceMismatch);
        }
        let mut b = self.create_binding(&source.view()?, options)?;
        b.managed = Some((
            source.clone(),
            Arc::new(Mutex::new((b.view.clone(), b.group.clone()))),
        ));
        Ok(b)
    }
    /// Replaces a snapshot source, retaining its sampler. Live tracking becomes a snapshot.
    /// Existing recorded draws retain their original source binding.
    pub fn rebind(&self, b: &mut TextureBinding, view: &wgpu::TextureView) -> Result<(), Error> {
        if !self.graphics.same_device(&b.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.validate_source(view, b.options.filter)?;
        if b.view != *view {
            b.group = bind(
                self.graphics.device(),
                self.layout(b.options.filter),
                view,
                &b.sampler,
            );
            b.view = view.clone();
        }
        b.managed = None;
        Ok(())
    }
    /// Prepares default shading for a target before rendering.
    pub fn prepare_for_target(
        &mut self,
        b: &TextureBinding,
        target: &RenderTarget<'_>,
    ) -> Result<(), Error> {
        if !target.graphics().same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(b, &target.render_format())
    }
    /// Prepares default shading from a complete attachment format.
    pub fn prepare(
        &mut self,
        binding: &TextureBinding,
        format: &crate::RenderFormat,
    ) -> Result<(), Error> {
        let material = self.material.clone();
        self.prepare_material(binding, &material, format)
    }
    /// Prepares the built-in variant used by [`Self::draw_prepared_transformed`].
    /// Ordinary dynamic/identity draws use the separate variant from [`Self::prepare`].
    pub fn prepare_transformed(
        &mut self,
        binding: &TextureBinding,
        format: &crate::RenderFormat,
    ) -> Result<(), Error> {
        let material = self.material.clone();
        self.pipeline_variant(
            binding,
            &material,
            format.single_color()?,
            format.sample_count,
            format.depth_stencil,
            true,
        )?;
        Ok(())
    }
    /// Prepares a material; raw shader diagnostics follow wgpu error reporting.
    pub fn prepare_material(
        &mut self,
        b: &TextureBinding,
        m: &TextureMaterial,
        format: &crate::RenderFormat,
    ) -> Result<(), Error> {
        let color = if format.colors.len() == 1 {
            format.colors[0]
        } else {
            None
        }
        .ok_or(Error::ExpectedSingleColor)?;
        self.pipeline(b, m, color, format.sample_count, format.depth_stencil)?;
        Ok(())
    }
    /// Captures pipeline validation errors before caching a custom material.
    /// Reuses variants verified by this method; unchecked variants are recreated
    /// under validation. A failed variant is removed from the cache.
    pub async fn try_prepare_material(
        &mut self,
        b: &TextureBinding,
        m: &TextureMaterial,
        format: &crate::RenderFormat,
    ) -> Result<(), Error> {
        if !b.graphics.same_device(&self.graphics) || !m.graphics.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        let key = PipelineKey {
            material: m.id,
            format: format.single_color()?,
            count: format.sample_count,
            depth_stencil: format.depth_stencil,
            filter: b.options.filter,
            alpha: b.options.alpha,
            transformed: false,
        };
        if self.pipelines.get(&key).is_some_and(|p| p.is_validated()) {
            return self.prepare_material(b, m, format);
        }
        // An unchecked handle may be invalid even if synchronous preparation returned Ok.
        self.pipelines.remove(&key);
        let scope = self
            .graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let original = std::mem::take(&mut self.pipelines);
        let result = self.prepare_material(b, m, format);
        let mut prepared = std::mem::replace(&mut self.pipelines, original);
        if let Some(e) = scope.pop().await {
            return Err(Error::Validation(e));
        }
        result?;
        for pipeline in prepared.values_mut() {
            pipeline.mark_validated();
        }
        self.pipelines.extend(prepared);
        Ok(())
    }
    /// Uploads immutable draw parameters once. Normalized rectangles can use any
    /// viewport size; pixel rectangles retain the supplied viewport size.
    /// Rejects empty data, invalid parameters, or data beyond device buffer limits.
    pub fn prepare_draws(
        &self,
        draws: &[TextureDraw],
        viewport: [f32; 2],
    ) -> Result<PreparedTextureDraw, Error> {
        use wgpu::util::DeviceExt;
        if draws.is_empty()
            || draws.len() > u32::MAX as usize
            || draws.len() as u64 > self.graphics.device().limits().max_buffer_size / 64
            || !viewport.iter().all(|v| v.is_finite() && *v > 0.)
        {
            return Err(Error::InvalidTextureDraw);
        }
        let parameters: Result<Vec<_>, _> = draws.iter().map(|d| d.parameters(viewport)).collect();
        let parameters = parameters?;
        let mut bounds: [Option<[f64; 4]>; 2] = [None, None];
        for p in &parameters {
            let index = usize::from(p.axis_y[2] != 0.);
            for [u, v] in [[0., 0.], [1., 0.], [0., 1.], [1., 1.]] {
                let x = f64::from(p.origin_axis_x[0])
                    + u * f64::from(p.origin_axis_x[2])
                    + v * f64::from(p.axis_y[0]);
                let y = f64::from(p.origin_axis_x[1])
                    + u * f64::from(p.origin_axis_x[3])
                    + v * f64::from(p.axis_y[1]);
                bounds[index] = Some(match bounds[index] {
                    None => [x, y, x, y],
                    Some(b) => [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)],
                });
            }
        }
        let buffer = self
            .graphics
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("RXUI drawing static texture draws"),
                contents: bytemuck::cast_slice(&parameters),
                usage: wgpu::BufferUsages::VERTEX,
            });
        Ok(PreparedTextureDraw {
            graphics: self.graphics.clone(),
            buffer,
            count: draws.len() as u32,
            bounds,
            viewport: draws.iter().any(|d| !d.normalized).then_some(viewport),
        })
    }

    /// Draws static prepared parameters with no recurring parameter uploads.
    pub fn draw_prepared(
        &mut self,
        pass: &mut RenderPass<'_>,
        binding: &TextureBinding,
        draws: &PreparedTextureDraw,
    ) -> Result<(), Error> {
        let material = self.material.clone();
        self.draw_prepared_with_material(pass, binding, &material, draws)
    }

    /// Draws retained placements with an additional local transform using built-in shading.
    ///
    /// The transform follows each instance's transform in its selected units,
    /// matching dynamic drawing and Painter scopes. Identity uses the ordinary
    /// prepared path with no uploads. Other transforms upload only 48 bytes;
    /// retained instance geometry is never copied or uploaded again. Pixel-space
    /// data still requires the viewport supplied during preparation.
    /// Invalid devices, viewports, transforms or bounds record no draw.
    pub fn draw_prepared_transformed(
        &mut self,
        pass: &mut RenderPass<'_>,
        binding: &TextureBinding,
        draws: &PreparedTextureDraw,
        transform: impl Into<crate::Transform2D>,
    ) -> Result<(), Error> {
        let transform = transform.into();
        if transform == crate::Transform2D::IDENTITY {
            return self.draw_prepared(pass, binding, draws);
        }
        if !draws.graphics.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        validate_prepared_viewport(pass, draws)?;
        let parameters = prepared_transform(draws, transform, pass.viewport_size())?;
        let material = self.material.clone();
        let group = self
            .validate_binding(pass, binding, &material)?
            .into_owned();
        let pipeline = self
            .pipeline_variant(
                binding,
                &material,
                pass.single_color_format()?,
                pass.sample_count(),
                pass.depth_stencil_format(),
                true,
            )?
            .clone();
        let (buffer, range) = pass.upload_instances(bytemuck::bytes_of(&parameters), 4);
        pass.apply_raster_state();
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, &draws.buffer, 0..draws.buffer.size());
        pass.set_vertex_buffer(1, &buffer, range);
        pass.bind_clip(2);
        pass.bind_rounded_clips();
        pass.draw(0..6, 0..draws.count);
        Ok(())
    }

    /// Draws static prepared parameters with an explicit reusable material.
    pub fn draw_prepared_with_material(
        &mut self,
        pass: &mut RenderPass<'_>,
        binding: &TextureBinding,
        material: &TextureMaterial,
        draws: &PreparedTextureDraw,
    ) -> Result<(), Error> {
        if !draws.graphics.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        validate_prepared_viewport(pass, draws)?;
        self.setup_draw(pass, binding, material)?;
        pass.set_vertex_buffer(0, &draws.buffer, 0..draws.buffer.size());
        pass.bind_clip(2);
        if material.shader.is_none() {
            pass.bind_rounded_clips();
        }
        pass.draw(0..6, 0..draws.count);
        Ok(())
    }

    /// Binds one image and default material for repeated dynamic or prepared draws.
    ///
    /// Device, feedback, and material checks and pipeline selection happen once.
    /// A live framebuffer source is snapshotted for this scope: begin a new scope
    /// to follow later storage replacement. Dynamic draw data still gets validated
    /// and uploaded through frame-owned pages. The scope borrows the renderer's
    /// reusable CPU scratch storage and the pass; it does not submit or end a pass.
    pub fn bind<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
        binding: &TextureBinding,
    ) -> Result<TextureDrawSession<'draw, 'frame>, Error> {
        let material = self.material.clone();
        self.bind_with_material(pass, binding, &material)
    }

    /// Binds one image and explicit material for scoped drawing.
    /// Errors match [`Self::draw_with_material`]; prepared pipeline hits allocate
    /// no GPU resources. Application groups beyond the image group remain caller-owned.
    pub fn bind_with_material<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
        binding: &TextureBinding,
        material: &TextureMaterial,
    ) -> Result<TextureDrawSession<'draw, 'frame>, Error> {
        let (pipeline, group) = self.bind_resources(pass, binding, material)?;
        Ok(TextureDrawSession {
            pass,
            pipeline,
            group,
            parameters: &mut self.parameters,
            builtin_clip: material.shader.is_none(),
            dirty: false,
        })
    }

    /// Binds an image and immutable instance data for repeated drawing in one scope.
    ///
    /// This is the direct route to [`PreparedTextureDrawSession`]; it does not
    /// require a parent image scope or keep the renderer borrowed. Fixed resources and pixel viewport compatibility
    /// are validated at binding. Subsequent pass access triggers revalidation and
    /// restoration before drawing. A live image source is snapshotted for the scope.
    pub fn bind_prepared<'draw, 'frame>(
        &mut self,
        pass: &'draw mut RenderPass<'frame>,
        binding: &TextureBinding,
        draws: &'draw PreparedTextureDraw,
    ) -> Result<PreparedTextureDrawSession<'draw, 'frame>, Error> {
        let material = self.material.clone();
        self.bind_prepared_with_material(pass, binding, &material, draws)
    }
    /// Binds an image, explicit material, and immutable instance data for scoped drawing.
    /// Errors match [`Self::draw_prepared_with_material`]. This performs no recurring uploads.
    pub fn bind_prepared_with_material<'draw, 'frame>(
        &mut self,
        pass: &'draw mut RenderPass<'frame>,
        binding: &TextureBinding,
        material: &TextureMaterial,
        draws: &'draw PreparedTextureDraw,
    ) -> Result<PreparedTextureDrawSession<'draw, 'frame>, Error> {
        if !draws.graphics.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        validate_prepared_viewport(pass, draws)?;
        let (pipeline, group) = self.bind_resources(pass, binding, material)?;
        pass.set_vertex_buffer(0, &draws.buffer, 0..draws.buffer.size());
        Ok(PreparedTextureDrawSession {
            pass,
            pipeline,
            group,
            builtin_clip: material.shader.is_none(),
            draws,
            dirty: false,
        })
    }

    /// Draws one rectangle using default premultiplied shading.
    pub fn draw(
        &mut self,
        pass: &mut RenderPass<'_>,
        b: &TextureBinding,
        draw: TextureDraw,
    ) -> Result<(), Error> {
        let material = self.material.clone();
        self.draw_with_material(pass, b, &material, draw)
    }
    /// Draws using explicit reusable material state, including stencil clipping.
    pub fn draw_with_material(
        &mut self,
        pass: &mut RenderPass<'_>,
        b: &TextureBinding,
        m: &TextureMaterial,
        draw: TextureDraw,
    ) -> Result<(), Error> {
        self.draw_many_with_material(pass, b, m, std::slice::from_ref(&draw))
    }
    /// Instances consecutive rectangles using one image and default material.
    /// Order follows the input slice. No automatic sorting is performed.
    pub fn draw_many(
        &mut self,
        pass: &mut RenderPass<'_>,
        b: &TextureBinding,
        draws: &[TextureDraw],
    ) -> Result<(), Error> {
        let material = self.material.clone();
        self.draw_many_with_material(pass, b, &material, draws)
    }
    /// Instances consecutive draws with explicit material state. Large slices split
    /// at upload-page boundaries while preserving order. Rejects invalid parameters
    /// before recording any draw.
    pub fn draw_many_with_material(
        &mut self,
        pass: &mut RenderPass<'_>,
        b: &TextureBinding,
        m: &TextureMaterial,
        draws: &[TextureDraw],
    ) -> Result<(), Error> {
        let viewport = pass.viewport_size();
        self.parameters.clear();
        for draw in draws {
            self.parameters.push(draw.parameters(viewport)?);
        }
        if self.parameters.is_empty() {
            return Ok(());
        }
        self.setup_draw(pass, b, m)?;
        record_parameters(pass, &self.parameters, m.shader.is_none());
        Ok(())
    }
    fn bind_resources(
        &mut self,
        pass: &mut RenderPass<'_>,
        binding: &TextureBinding,
        material: &TextureMaterial,
    ) -> Result<(wgpu::RenderPipeline, wgpu::BindGroup), Error> {
        let group = self.validate_binding(pass, binding, material)?.into_owned();
        let pipeline = self
            .pipeline(
                binding,
                material,
                pass.single_color_format()?,
                pass.sample_count(),
                pass.depth_stencil_format(),
            )?
            .clone();
        pass.apply_raster_state();
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        Ok((pipeline, group))
    }
    fn setup_draw(
        &mut self,
        pass: &mut RenderPass<'_>,
        b: &TextureBinding,
        m: &TextureMaterial,
    ) -> Result<(), Error> {
        let group = self.validate_binding(pass, b, m)?;
        let pipeline = self.pipeline(
            b,
            m,
            pass.single_color_format()?,
            pass.sample_count(),
            pass.depth_stencil_format(),
        )?;
        pass.apply_raster_state();
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        Ok(())
    }

    fn validate_binding<'binding>(
        &self,
        pass: &RenderPass<'_>,
        b: &'binding TextureBinding,
        m: &TextureMaterial,
    ) -> Result<Cow<'binding, wgpu::BindGroup>, Error> {
        if !pass.same_device(&self.graphics)
            || !b.graphics.same_device(&self.graphics)
            || !m.graphics.same_device(&self.graphics)
        {
            return Err(Error::DeviceMismatch);
        }
        let (view, group) = if let Some((source, cache)) = &b.managed {
            let view = source.view()?;
            let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.0 != view {
                self.validate_source(&view, b.options.filter)?;
                *cache = (
                    view.clone(),
                    bind(
                        self.graphics.device(),
                        self.layout(b.options.filter),
                        &view,
                        &b.sampler,
                    ),
                );
            }
            (Cow::Owned(view), Cow::Owned(cache.1.clone()))
        } else {
            (Cow::Borrowed(&b.view), Cow::Borrowed(&b.group))
        };
        if pass.uses_color_texture(view.texture()) {
            return Err(Error::TextureFeedback);
        }
        if let Some(state) = &m.depth_stencil {
            if pass.depth_read_only() && !state.is_depth_read_only() {
                return Err(Error::ReadOnlyDepth);
            }
            if pass.stencil_read_only() && !state.is_stencil_read_only(m.cull_mode) {
                return Err(Error::ReadOnlyStencil);
            }
        }
        Ok(group)
    }

    fn validate_source(
        &self,
        view: &wgpu::TextureView,
        filter: TextureFilter,
    ) -> Result<(), Error> {
        let texture = view.texture();
        let format = texture.format();
        let features = crate::target::format_features(&self.graphics, format);
        if texture.sample_count() != 1
            || !texture
                .usage()
                .contains(wgpu::TextureUsages::TEXTURE_BINDING)
            || !matches!(
                format.sample_type(None, Some(self.graphics.device().features())),
                Some(wgpu::TextureSampleType::Float { .. })
            )
            || filter == TextureFilter::Linear
                && !features
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
        {
            return Err(Error::InvalidTextureSource { format });
        }
        Ok(())
    }
    fn pipeline(
        &mut self,
        b: &TextureBinding,
        m: &TextureMaterial,
        format: wgpu::TextureFormat,
        count: u32,
        depth_stencil: Option<wgpu::TextureFormat>,
    ) -> Result<&wgpu::RenderPipeline, Error> {
        self.pipeline_variant(b, m, format, count, depth_stencil, false)
    }
    #[allow(clippy::too_many_arguments)]
    fn pipeline_variant(
        &mut self,
        b: &TextureBinding,
        m: &TextureMaterial,
        format: wgpu::TextureFormat,
        count: u32,
        depth_stencil: Option<wgpu::TextureFormat>,
        transformed: bool,
    ) -> Result<&wgpu::RenderPipeline, Error> {
        if !b.graphics.same_device(&self.graphics) || !m.graphics.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        if let Some(state) = &m.depth_stencil
            && Some(state.format) != depth_stencil
        {
            return Err(Error::DepthStencilMismatch {
                material: state.format,
                pass: depth_stencil,
            });
        }
        let key = PipelineKey {
            material: m.id,
            format,
            count,
            depth_stencil,
            filter: b.options.filter,
            alpha: b.options.alpha,
            transformed,
        };
        let entry = match self.pipelines.entry(key) {
            Entry::Occupied(entry) => return Ok(entry.into_mut()),
            Entry::Vacant(entry) => entry,
        };
        {
            let features = crate::target::format_features(&self.graphics, format);
            if !self
                .graphics
                .device()
                .features()
                .contains(format.required_features())
                || !features
                    .allowed_usages
                    .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
                || !matches!(
                    format.sample_type(None, Some(self.graphics.device().features())),
                    Some(wgpu::TextureSampleType::Float { .. })
                )
            {
                return Err(Error::UnsupportedColorFormat { format });
            }
            if m.blend.is_some()
                && !features
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
            {
                return Err(Error::UnsupportedMaterialFormat { format });
            }
            crate::target::validate_sample_count(
                format,
                count,
                &crate::target::sample_counts(features),
            )?;
            if let Some(depth_format) = depth_stencil {
                let counts = crate::depth_stencil::supported_counts(
                    &self.graphics,
                    depth_format,
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                )?;
                crate::target::validate_sample_count(depth_format, count, &counts)?;
            }
            let mut layouts = vec![Some(match b.options.filter {
                TextureFilter::Nearest => &self.nearest,
                TextureFilter::Linear => &self.linear,
            })];
            let rounded_layout = self.graphics.rounded_clip_layout();
            if m.shader.is_none() {
                if self.graphics.rounded_clip_capacity() > 0 {
                    layouts.push(Some(&rounded_layout));
                }
            } else {
                layouts.extend(m.layouts.iter().map(|v| v.as_ref()));
            }
            let layout =
                self.graphics
                    .device()
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("RXUI drawing texture material"),
                        bind_group_layouts: &layouts,
                        immediate_size: 0,
                    });
            let shader = m.shader.as_ref().unwrap_or(&self.shader);
            let pipeline =
                self.graphics
                    .device()
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("RXUI drawing texture pipeline"),
                        layout: Some(&layout),
                        vertex: wgpu::VertexState {
                            module: shader,
                            entry_point: Some(if transformed {
                                "vertex_transformed"
                            } else {
                                &m.vertex_entry
                            }),
                            compilation_options: Default::default(),
                            buffers: &[
                                Some(parameter_layout()),
                                transformed.then(transform_layout),
                                Some(clip_layout()),
                            ],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: shader,
                            entry_point: Some(if m.shader.is_some() {
                                &m.fragment_entry
                            } else {
                                let covered = m.depth_stencil.as_ref().is_some_and(|state| {
                                    !state.is_depth_read_only()
                                        || !state.is_stencil_read_only(m.cull_mode)
                                });
                                match (b.options.alpha, b.options.filter, covered) {
                                    (TextureAlpha::Straight, TextureFilter::Nearest, false) => {
                                        "fragment_straight"
                                    }
                                    (TextureAlpha::Straight, TextureFilter::Nearest, true) => {
                                        "fragment_straight_covered"
                                    }
                                    (TextureAlpha::Straight, TextureFilter::Linear, false) => {
                                        "fragment_straight_linear"
                                    }
                                    (TextureAlpha::Straight, TextureFilter::Linear, true) => {
                                        "fragment_straight_linear_covered"
                                    }
                                    (TextureAlpha::Premultiplied, _, false) => {
                                        "fragment_premultiplied"
                                    }
                                    (TextureAlpha::Premultiplied, _, true) => {
                                        "fragment_premultiplied_covered"
                                    }
                                }
                            }),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend: m.blend,
                                write_mask: m.write_mask,
                            })],
                        }),
                        primitive: wgpu::PrimitiveState {
                            front_face: m.front_face,
                            cull_mode: m.cull_mode,
                            ..Default::default()
                        },
                        depth_stencil: m.depth_stencil.clone().or_else(|| {
                            depth_stencil.map(|format| wgpu::DepthStencilState {
                                format,
                                depth_write_enabled: format.has_depth_aspect().then_some(false),
                                depth_compare: None,
                                stencil: Default::default(),
                                bias: Default::default(),
                            })
                        }),
                        multisample: wgpu::MultisampleState {
                            count,
                            ..Default::default()
                        },
                        multiview_mask: None,
                        cache: None,
                    });
            Ok(entry.insert(crate::pipeline_cache::CachedPipeline::new(
                pipeline,
                &m.lifetime,
            )))
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PreparedTransform {
    pixels: [f32; 4],
    normalized: [f32; 4],
    translations: [f32; 4],
}
fn clip_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![7=>Float32x4,8=>Float32x4,9=>Float32x4];
    wgpu::VertexBufferLayout {
        array_stride: 0,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}
fn transform_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![4=>Float32x4,5=>Float32x4,6=>Float32x4];
    wgpu::VertexBufferLayout {
        array_stride: 0,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}
fn prepared_transform(
    draws: &PreparedTextureDraw,
    transform: crate::Transform2D,
    viewport: [f32; 2],
) -> Result<PreparedTransform, Error> {
    if !transform.valid() {
        return Err(Error::InvalidTransform2D);
    }
    let [a, b, c, d, tx, ty] = transform.0.map(f64::from);
    let [w, h] = viewport.map(|v| if v > 0. { f64::from(v) } else { 1. });
    let p = PreparedTransform {
        pixels: [a as f32, (b * w / h) as f32, (c * h / w) as f32, d as f32],
        normalized: [a as f32, b as f32, c as f32, d as f32],
        translations: [(tx / w) as f32, (ty / h) as f32, tx as f32, ty as f32],
    };
    if bytemuck::cast_slice::<PreparedTransform, f32>(&[p])
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err(Error::InvalidTransform2D);
    }
    for (index, bounds) in draws.bounds.iter().enumerate() {
        if let Some(bounds) = bounds {
            let m = if index == 0 { p.pixels } else { p.normalized }.map(f64::from);
            let offset = if index == 0 {
                [p.translations[0], p.translations[1]]
            } else {
                [p.translations[2], p.translations[3]]
            }
            .map(f64::from);
            for x in [bounds[0], bounds[2]] {
                for y in [bounds[1], bounds[3]] {
                    let q = [
                        m[0] * x + m[2] * y + offset[0],
                        m[1] * x + m[3] * y + offset[1],
                    ];
                    if q.iter().any(|v| !(*v as f32 * 2.).is_finite()) {
                        return Err(Error::InvalidTextureDraw);
                    }
                }
            }
        }
    }
    Ok(p)
}

fn binding_layout(device: &wgpu::Device, filtering: bool) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("RXUI drawing image source"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float {
                        filterable: filtering,
                    },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Sampler(if filtering {
                    wgpu::SamplerBindingType::Filtering
                } else {
                    wgpu::SamplerBindingType::NonFiltering
                }),
                count: None,
            },
        ],
    })
}
fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("RXUI drawing image binding"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
#[cfg(test)]
#[path = "texture_tests.rs"]
mod tests;
