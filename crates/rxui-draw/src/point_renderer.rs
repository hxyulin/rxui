use crate::{
    DrawSpace, EdgeAntialiasing, Error, GraphicsContext, LineCap, LineJoin, Material,
    MaterialOptions, PipelineOptions, PointBuffer, RenderFormat, RenderPass, Transform2D,
    VertexLayout,
};
use bytemuck::{Pod, Zeroable};
use std::ops::Range;

/// Placement/style for a connected polyline over a retained point range.
///
/// Coordinates transform on the GPU; width stays in physical screen pixels.
/// Ranges are logical point indices, including gaps, and selected endpoints get
/// caps. To clip a continuing curve, include neighboring samples and use scissor.
/// Connected miter/bevel/round joins avoid ordinary shared-edge overblending, but
/// self-intersections, reversals, and very short/sharp segments can overlap. This
/// fast renderer does not union strokes like PreparedPath. Round joins use eight
/// arc segments. Coverage is approximate; target MSAA remains independently chosen.
#[derive(Clone, Debug)]
pub struct PolylineDraw {
    /// Linear straight RGBA; shader output is premultiplied.
    pub color: [f32; 4],
    /// Original sample units before affine transformation/viewport conversion.
    pub space: DrawSpace,
    /// Data-to-destination transform, including axis inversion if desired.
    pub transform: Transform2D,
    /// Finite nonnegative physical-pixel width. Zero draws no area.
    pub width_pixels: f32,
    /// Open-run caps, including at gaps and selected range ends.
    pub cap: LineCap,
    /// Connections between adjacent nondegenerate segments.
    pub join: LineJoin,
    /// Finite limit >=1, relative to half-width; exceeded outer miters bevel.
    pub miter_limit: f32,
    /// Approximate edge filtering or geometric sample coverage through target MSAA.
    pub antialiasing: EdgeAntialiasing,
    /// Logical selected points, or None for the full resource.
    pub range: Option<Range<usize>>,
}
impl Default for PolylineDraw {
    fn default() -> Self {
        Self::new([1.; 4])
    }
}
impl PolylineDraw {
    /// White/default geometry placement uses identity pixels, width one, butt/miter, coverage.
    pub const fn new(color: [f32; 4]) -> Self {
        Self {
            color,
            space: DrawSpace::Pixels,
            transform: Transform2D::IDENTITY,
            width_pixels: 1.,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.,
            antialiasing: EdgeAntialiasing::Coverage,
            range: None,
        }
    }
    /// Selects physical-pixel width without updating samples.
    pub const fn width_pixels(mut self, width: f32) -> Self {
        self.width_pixels = width;
        self
    }
    /// Selects cap shape at each open run.
    pub const fn cap(mut self, cap: LineCap) -> Self {
        self.cap = cap;
        self
    }
    /// Selects connected join geometry.
    pub const fn join(mut self, join: LineJoin) -> Self {
        self.join = join;
        self
    }
    /// Selects a miter limit relative to half-width.
    pub const fn miter_limit(mut self, limit: f32) -> Self {
        self.miter_limit = limit;
        self
    }
    /// Selects GPU sample placement; screen-pixel width does not scale with it.
    pub fn transform(mut self, t: impl Into<Transform2D>) -> Self {
        self.transform = t.into();
        self
    }
    /// Selects original sample units before GPU placement.
    pub const fn space(mut self, space: DrawSpace) -> Self {
        self.space = space;
        self
    }
    /// Selects edge filtering independently of MSAA.
    pub const fn antialiasing(mut self, aa: EdgeAntialiasing) -> Self {
        self.antialiasing = aa;
        self
    }
    /// Selects logical points; gaps/ring ordering retain their meaning.
    pub fn range(mut self, range: Range<usize>) -> Self {
        self.range = Some(range);
        self
    }
    /// Selects linear straight RGBA without point uploads.
    pub const fn color(mut self, color: [f32; 4]) -> Self {
        self.color = color;
        self
    }
}

/// Screen-pixel marker geometry, independent of sample density and zoom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkerShape {
    /// Circular marker, the default.
    #[default]
    Circle,
    /// Axis-aligned screen square.
    Square,
    /// Screen diamond with vertices on the radius axes.
    Diamond,
}
/// Per-series placement and screen-pixel markers over retained samples.
/// Markers overlap/blend in logical order. Gaps are skipped. There is no CPU
/// expansion, sample scan, or automatic culling/reduction during drawing.
#[derive(Clone, Debug)]
pub struct MarkerDraw {
    /// Linear straight RGBA.
    pub color: [f32; 4],
    /// Original sample units.
    pub space: DrawSpace,
    /// GPU data-to-destination transform; radius remains in screen pixels.
    pub transform: Transform2D,
    /// Finite nonnegative physical-pixel radius. Zero draws no area.
    pub radius_pixels: f32,
    /// Circle, square, or diamond, in screen coordinates.
    pub shape: MarkerShape,
    /// Approximate analytic edges, or hard edges with independently chosen MSAA.
    pub antialiasing: EdgeAntialiasing,
    /// Logical point range, or None for the full buffer.
    pub range: Option<Range<usize>>,
}
impl Default for MarkerDraw {
    fn default() -> Self {
        Self::new([1.; 4])
    }
}
impl MarkerDraw {
    /// Identity pixel placement, radius two, circles, and edge coverage.
    pub const fn new(color: [f32; 4]) -> Self {
        Self {
            color,
            space: DrawSpace::Pixels,
            transform: Transform2D::IDENTITY,
            radius_pixels: 2.,
            shape: MarkerShape::Circle,
            antialiasing: EdgeAntialiasing::Coverage,
            range: None,
        }
    }
    /// Selects physical-pixel radius without changing data.
    pub const fn radius_pixels(mut self, radius: f32) -> Self {
        self.radius_pixels = radius;
        self
    }
    /// Selects screen-space marker geometry.
    pub const fn shape(mut self, shape: MarkerShape) -> Self {
        self.shape = shape;
        self
    }
    /// Selects GPU sample placement; radius does not scale.
    pub fn transform(mut self, t: impl Into<Transform2D>) -> Self {
        self.transform = t.into();
        self
    }
    /// Selects original sample units.
    pub const fn space(mut self, space: DrawSpace) -> Self {
        self.space = space;
        self
    }
    /// Selects edge coverage, independently of target MSAA.
    pub const fn antialiasing(mut self, aa: EdgeAntialiasing) -> Self {
        self.antialiasing = aa;
        self
    }
    /// Selects logical samples, including gaps.
    pub fn range(mut self, range: Range<usize>) -> Self {
        self.range = Some(range);
        self
    }
    /// Selects linear straight RGBA without point uploads.
    pub const fn color(mut self, color: [f32; 4]) -> Self {
        self.color = color;
        self
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Parameters {
    axes: [f32; 4],
    placement: [f32; 4],
    color: [f32; 4],
    style: [f32; 4],
    range: [u32; 4],
}
#[derive(Clone, Copy)]
struct Style {
    color: [f32; 4],
    space: DrawSpace,
    t: Transform2D,
    size: f32,
    aa: EdgeAntialiasing,
    aux: f32,
    mode: f32,
    stride: u32,
}
fn parameters(
    points: &PointBuffer,
    range: Option<Range<usize>>,
    s: Style,
    viewport: [f32; 2],
    markers: bool,
) -> Result<(Parameters, u32), Error> {
    let r = range.unwrap_or(0..points.len());
    if r.start > r.end || r.end > points.len() {
        return Err(Error::InvalidPointRange);
    }
    if !s.t.valid()
        || !crate::drawing::color_valid(s.color)
        || !s.size.is_finite()
        || s.size < 0.
        || !s.aux.is_finite()
        || (!markers && s.aux < 1.)
    {
        return Err(Error::InvalidPointDraw);
    }
    let count = r.end - r.start;
    let elements = if markers {
        count
    } else {
        count.saturating_sub(1)
    };
    let [vx, vy] = viewport.map(f64::from);
    if vx == 0. || vy == 0. || s.size == 0. || elements == 0 {
        return Ok((Parameters::zeroed(), 0));
    }
    let [a, b, c, d, x, y] = s.t.0.map(f64::from);
    let [sx, sy] = if s.space == DrawSpace::Pixels {
        [1., 1.]
    } else {
        [vx, vy]
    };
    let axes = [a * sx, b * sy, c * sx, d * sy];
    let translation = [x * sx, y * sy];
    let padding = f64::from(s.size)
        * (if markers {
            1.
        } else {
            f64::from(s.aux.max(1.))
        })
        + 2.;
    for (i, v) in [vx, vy].into_iter().enumerate() {
        let extent = axes[i].abs() * points.extent[0]
            + axes[i + 2].abs() * points.extent[1]
            + translation[i].abs()
            + padding;
        if !extent.is_finite()
            || extent > f64::from(f32::MAX) * 0.0625
            || extent / v > f64::from(f32::MAX) * 0.0625
        {
            return Err(Error::InvalidPointDraw);
        }
    }
    if axes[0] * axes[3] - axes[1] * axes[2] == 0. && !markers {
        return Ok((Parameters::zeroed(), 0));
    }
    let p = Parameters {
        axes: axes.map(|v| v as f32),
        placement: [
            translation[0] as f32,
            translation[1] as f32,
            viewport[0],
            viewport[1],
        ],
        color: s.color,
        style: [
            s.size,
            s.aux,
            if s.aa == EdgeAntialiasing::Coverage {
                1.
            } else {
                0.
            },
            s.mode,
        ],
        range: [
            ((points.head + r.start) % points.capacity()) as u32,
            count as u32,
            points.capacity() as u32,
            s.stride,
        ],
    };
    if p.axes
        .into_iter()
        .chain(p.placement)
        .chain(p.color)
        .chain(p.style)
        .any(|v| !v.is_finite())
    {
        return Err(Error::InvalidPointDraw);
    }
    let vertices = (elements as u32)
        .checked_mul(s.stride)
        .ok_or(Error::InvalidPointDraw)?;
    Ok((p, vertices))
}
fn polyline(
    points: &PointBuffer,
    draw: PolylineDraw,
    viewport: [f32; 2],
) -> Result<(Parameters, u32), Error> {
    let join = match draw.join {
        LineJoin::Miter => 0,
        LineJoin::Bevel => 1,
        LineJoin::Round => 2,
    };
    let cap = match draw.cap {
        LineCap::Butt => 0,
        LineCap::Square => 1,
        LineCap::Round => 2,
    };
    let stride = (if join == 2 { 30 } else { 9 }) + if cap == 2 { 12 } else { 0 };
    parameters(
        points,
        draw.range,
        Style {
            color: draw.color,
            space: draw.space,
            t: draw.transform,
            size: draw.width_pixels,
            aa: draw.antialiasing,
            aux: draw.miter_limit,
            mode: (join * 4 + cap) as f32,
            stride,
        },
        viewport,
        false,
    )
}
fn markers(
    points: &PointBuffer,
    draw: MarkerDraw,
    viewport: [f32; 2],
) -> Result<(Parameters, u32), Error> {
    parameters(
        points,
        draw.range,
        Style {
            color: draw.color,
            space: draw.space,
            t: draw.transform,
            size: draw.radius_pixels,
            aa: draw.antialiasing,
            aux: match draw.shape {
                MarkerShape::Circle => 0.,
                MarkerShape::Square => 1.,
                MarkerShape::Diamond => 2.,
            },
            mode: 0.,
            stride: 6,
        },
        viewport,
        true,
    )
}

#[derive(Debug)]
struct Renderer {
    graphics: GraphicsContext,
    options: PipelineOptions,
    material: Option<Material>,
    pipelines: crate::pipelines::Pipelines,
    markers: bool,
}
impl Renderer {
    fn new(g: &GraphicsContext, options: PipelineOptions, markers: bool) -> Self {
        Self {
            graphics: g.clone(),
            options,
            material: None,
            pipelines: Default::default(),
            markers,
        }
    }
    fn material(&mut self) -> Result<(), Error> {
        if self.material.is_none() {
            crate::points::check_support(&self.graphics)?;
            let shader =
                self.graphics
                    .device()
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("RXUI drawing dense points"),
                        source: wgpu::ShaderSource::Wgsl(
                            crate::clip::shader(&self.graphics, include_str!("points.wgsl")).into(),
                        ),
                    });
            let layout = VertexLayout {
                stride: 80,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: wgpu::vertex_attr_array![
                    0 => Float32x4, 1 => Float32x4, 2 => Float32x4,
                    3 => Float32x4, 4 => Uint32x4
                ]
                .to_vec(),
            };
            self.material = Some(
                self.graphics.create_material(
                    self.options.mesh(
                        MaterialOptions::new(&shader)
                            .vertex_layouts(&[layout])
                            .bind_group_layouts(&crate::clip::layouts(
                                &self.graphics,
                                Some(&crate::points::layout(self.graphics.device())),
                                &self.graphics.rounded_clip_layout(),
                            ))
                            .entry_points(
                                if self.markers {
                                    "marker_vertex"
                                } else {
                                    "polyline_vertex"
                                },
                                if self.options.writes_attachment() {
                                    "fragment_covered"
                                } else {
                                    "fragment_main"
                                },
                            ),
                    ),
                ),
            );
        }
        Ok(())
    }
    fn pipeline(
        &mut self,
        color: wgpu::TextureFormat,
        samples: u32,
        depth: Option<wgpu::TextureFormat>,
    ) -> Result<wgpu::RenderPipeline, Error> {
        self.material()?;
        Ok(crate::pipelines::pipeline(
            &self.graphics,
            &mut self.pipelines,
            self.material.as_ref().unwrap(),
            color,
            samples,
            depth,
            true,
        )?
        .clone())
    }
    fn prepare(&mut self, f: &RenderFormat) -> Result<(), Error> {
        self.pipeline(f.single_color()?, f.sample_count, f.depth_stencil)?;
        Ok(())
    }
    fn bind<'a, 'f>(
        &mut self,
        pass: &'a mut RenderPass<'f>,
        points: &'a PointBuffer,
    ) -> Result<Session<'a, 'f>, Error> {
        if !pass.same_device(&self.graphics) || !points.graphics.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.material()?;
        crate::pipelines::validate_aspects(pass, self.material.as_ref().unwrap())?;
        let pipeline = self.pipeline(
            pass.single_color_format()?,
            pass.sample_count(),
            pass.depth_stencil_format(),
        )?;
        Ok(Session {
            pass,
            points,
            pipeline,
        })
    }
}
#[derive(Debug)]
struct Session<'a, 'f> {
    pass: &'a mut RenderPass<'f>,
    points: &'a PointBuffer,
    pipeline: wgpu::RenderPipeline,
}
impl Session<'_, '_> {
    fn record(&mut self, p: Parameters, vertices: u32) {
        if vertices == 0 {
            return;
        }
        self.pass.apply_raster_state();
        self.pass.set_pipeline(&self.pipeline);
        self.pass.set_bind_group(0, &self.points.group, &[]);
        self.pass.bind_rounded_clips();
        let first = self.pass.bind_instances(0, bytemuck::bytes_of(&p), 80);
        self.pass.draw(0..vertices, first..first + 1);
    }
}

/// GPU-expanded connected strokes over retained samples, without CPU tessellation.
///
/// Pipeline creation is lazy; prepare during loading for predictable redraw cost.
/// Requires vertex storage support. Draws upload 80 bytes per series regardless
/// of selected sample count, and bind group zero belongs to this renderer.
/// Applications retain original data, choose visible ranges/reduction, and own
/// passes, clipping, updates, and submission. This renderer does not impose axes,
/// chart layout, hit testing, or a data-processing pipeline.
#[derive(Debug)]
pub struct PolylineRenderer {
    inner: Renderer,
}
impl PolylineRenderer {
    /// Creates lazy default premultiplied shading with inert depth/stencil.
    pub fn new(g: &GraphicsContext) -> Self {
        Self::with_options(g, PipelineOptions::default())
    }
    /// Selects immutable blending/color writes/depth/stencil policy.
    pub fn with_options(g: &GraphicsContext, options: PipelineOptions) -> Self {
        Self {
            inner: Renderer::new(g, options, false),
        }
    }
    /// Prepares attachments/MSAA and checks required point-rendering capabilities.
    pub fn prepare(&mut self, f: &RenderFormat) -> Result<(), Error> {
        self.inner.prepare(f)
    }
    /// Prepares a surface variant, rejecting foreign devices.
    pub fn prepare_for_target(&mut self, t: &crate::RenderTarget<'_>) -> Result<(), Error> {
        if !t.graphics().same_device(&self.inner.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(&t.render_format())
    }
    /// Draws one selected logical range in one GPU command. Invalid draws upload no parameters.
    pub fn draw(
        &mut self,
        p: &mut RenderPass<'_>,
        points: &PointBuffer,
        draw: PolylineDraw,
    ) -> Result<(), Error> {
        self.bind(p, points)?.draw(draw)
    }
    /// Selects one point resource/pipeline for repeated ranges/styles in this pass.
    pub fn bind<'a, 'f>(
        &mut self,
        p: &'a mut RenderPass<'f>,
        points: &'a PointBuffer,
    ) -> Result<PolylineDrawSession<'a, 'f>, Error> {
        Ok(PolylineDrawSession {
            inner: self.inner.bind(p, points)?,
        })
    }
}
/// Borrowed polyline resource/pass scope; pass access restores state on subsequent drawing.
#[derive(Debug)]
pub struct PolylineDrawSession<'a, 'f> {
    inner: Session<'a, 'f>,
}
impl<'f> PolylineDrawSession<'_, 'f> {
    /// Records a selected logical span and screen-space style without sample uploads.
    pub fn draw(&mut self, draw: PolylineDraw) -> Result<(), Error> {
        let (p, n) = polyline(self.inner.points, draw, self.inner.pass.viewport_size())?;
        self.inner.record(p, n);
        Ok(())
    }
    /// Borrows the pass for clipping, other renderers, or raw GPU work.
    pub fn pass(&mut self) -> &mut RenderPass<'f> {
        self.inner.pass
    }
}
/// GPU-expanded screen-space markers over the same retained point buffers as polylines.
/// Drawing uploads 80 bytes per series, skips gaps, preserves overlap order, and
/// performs no CPU scan or geometry expansion. Vertex storage support is checked
/// during preparation/drawing. Bind group zero belongs to this renderer.
#[derive(Debug)]
pub struct MarkerRenderer {
    inner: Renderer,
}
impl MarkerRenderer {
    /// Creates lazy default premultiplied shading with inert depth/stencil.
    pub fn new(g: &GraphicsContext) -> Self {
        Self::with_options(g, PipelineOptions::default())
    }
    /// Selects immutable blend/color writes/depth/stencil policy.
    pub fn with_options(g: &GraphicsContext, options: PipelineOptions) -> Self {
        Self {
            inner: Renderer::new(g, options, true),
        }
    }
    /// Prepares attachment/MSAA compatibility and required capabilities.
    pub fn prepare(&mut self, f: &RenderFormat) -> Result<(), Error> {
        self.inner.prepare(f)
    }
    /// Prepares a surface variant, rejecting foreign devices.
    pub fn prepare_for_target(&mut self, t: &crate::RenderTarget<'_>) -> Result<(), Error> {
        if !t.graphics().same_device(&self.inner.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(&t.render_format())
    }
    /// Draws one selected logical range with screen-pixel markers.
    pub fn draw(
        &mut self,
        p: &mut RenderPass<'_>,
        points: &PointBuffer,
        draw: MarkerDraw,
    ) -> Result<(), Error> {
        self.bind(p, points)?.draw(draw)
    }
    /// Borrows one resource/pass for repeated selected ranges and marker styles.
    pub fn bind<'a, 'f>(
        &mut self,
        p: &'a mut RenderPass<'f>,
        points: &'a PointBuffer,
    ) -> Result<MarkerDrawSession<'a, 'f>, Error> {
        Ok(MarkerDrawSession {
            inner: self.inner.bind(p, points)?,
        })
    }
}
/// Borrowed marker resource/pass scope, restoring required state after pass access.
#[derive(Debug)]
pub struct MarkerDrawSession<'a, 'f> {
    inner: Session<'a, 'f>,
}
impl<'f> MarkerDrawSession<'_, 'f> {
    /// Draws selected samples with physical-pixel markers and no sample uploads.
    pub fn draw(&mut self, draw: MarkerDraw) -> Result<(), Error> {
        let (p, n) = markers(self.inner.points, draw, self.inner.pass.viewport_size())?;
        self.inner.record(p, n);
        Ok(())
    }
    /// Borrows the pass for clipping, other renderers, or raw commands.
    pub fn pass(&mut self) -> &mut RenderPass<'f> {
        self.inner.pass
    }
}

#[cfg(test)]
mod tests;
