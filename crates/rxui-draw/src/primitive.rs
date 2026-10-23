use crate::{
    DrawSpace, EdgeAntialiasing, Error, GraphicsContext, LineCap, LineDraw, Material,
    MaterialOptions, Rect, RenderFormat, RenderPass, Shape, ShapeDraw, Transform2D, VertexLayout,
};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Parameters {
    origin_axis_x: [f32; 4],
    axis_y_min: [f32; 4],
    span_style: [f32; 4],
    geometry: [f32; 4],
    color: [f32; 4],
    // Per-corner radii [top-left, top-right, bottom-right, bottom-left] for rounded shapes.
    radii: [f32; 4],
}
const STRIDE: u64 = std::mem::size_of::<Parameters>() as u64;
const BRUSH_STRIDE: u64 = std::mem::size_of::<BrushParameters>() as u64;
const PAGE_INSTANCES: u64 = 64 * 1024 / STRIDE; // Records fit a 64 KiB upload page.

trait PrimitiveData: Copy {
    fn parameters(self, viewport: [f32; 2]) -> Result<Parameters, Error>;
    fn object_frame(self) -> ([f32; 2], [f32; 2], [f32; 2]);
}
impl PrimitiveData for ShapeDraw {
    fn object_frame(self) -> ([f32; 2], [f32; 2], [f32; 2]) {
        (
            [
                self.rect.x + self.rect.width * 0.5,
                self.rect.y + self.rect.height * 0.5,
            ],
            [1., 0.],
            [0., 1.],
        )
    }
    fn parameters(self, viewport: [f32; 2]) -> Result<Parameters, Error> {
        self.validate()?;
        let Rect {
            x,
            y,
            width,
            height,
        } = self.rect;
        if width == 0. || height == 0. || self.stroke.is_some_and(|s| s.width == 0.) {
            return Ok(Parameters::zeroed());
        }
        let mut radius = 0.;
        let (mut kind, mut radii) = match self.shape {
            Shape::Rectangle => (0., [0.; 4]),
            Shape::RoundedRectangle { radii } => {
                let radii = radii.fitted(width, height);
                (if radii == [0.; 4] { 0. } else { 1. }, radii)
            }
            Shape::Ellipse => (2., [0.; 4]),
            Shape::Shadow { radii, blur } => {
                // The shader stores the Gaussian's standard deviation.
                radius = blur * 0.5;
                (9., radii.fitted(width, height))
            }
        };
        let half_size = [width * 0.5, height * 0.5];
        // Three standard deviations hold all but 0.3% of the blurred coverage.
        let blur_extent = if kind == 9. { 3. * radius } else { 0. };
        let mut bounds = half_size.map(|v| v + blur_extent);
        let mut geometry = half_size;
        let mut stroke_width = 0.;
        if let Some(stroke) = self.stroke {
            let (_, outset) = stroke.offsets();
            bounds = [half_size[0] + outset, half_size[1] + outset];
            if matches!(self.shape, Shape::Ellipse) {
                // Ellipses use a distance band around the original curve.
                radius = outset;
            } else {
                // Rectangle contours remain rectangles; rounded corner radii
                // grow/shrink with the contour, clamping the inner radius to zero.
                // Sharp corners stay sharp.
                geometry = bounds;
                radii = radii.map(|r| if r > 0. { r + outset } else { 0. });
            }
            kind += 6.;
            stroke_width = stroke.width;
        }
        pack(
            [x + width * 0.5, y + height * 0.5],
            [1., 0.],
            [0., 1.],
            bounds,
            geometry,
            kind,
            radius,
            stroke_width,
            self.color,
            self.space,
            self.transform,
            self.antialiasing,
            viewport,
        )
        .map(|p| Parameters { radii, ..p })
        .ok_or(Error::InvalidShapeDraw)
    }
}
impl PrimitiveData for LineDraw {
    fn object_frame(self) -> ([f32; 2], [f32; 2], [f32; 2]) {
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        let length = dx.hypot(dy);
        let axis = if length == 0. {
            [1., 0.]
        } else {
            [dx / length, dy / length]
        };
        (
            [self.start[0] + dx * 0.5, self.start[1] + dy * 0.5],
            axis,
            [-axis[1], axis[0]],
        )
    }
    fn parameters(self, viewport: [f32; 2]) -> Result<Parameters, Error> {
        self.validate()?;
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        let length = dx.hypot(dy);
        if !length.is_finite() {
            return Err(Error::InvalidLineDraw);
        }
        if self.width == 0. || (length == 0. && self.cap == LineCap::Butt) {
            return Ok(Parameters::zeroed());
        }
        let axis = if length == 0. {
            [1., 0.]
        } else {
            [dx / length, dy / length]
        };
        let half_width = self.width * 0.5;
        let kind = match self.cap {
            LineCap::Butt => 3.,
            LineCap::Square => 4.,
            LineCap::Round => 5.,
        };
        let extent = length * 0.5
            + if self.cap == LineCap::Butt {
                0.
            } else {
                half_width
            };
        pack(
            [self.start[0] + dx * 0.5, self.start[1] + dy * 0.5],
            axis,
            [-axis[1], axis[0]],
            [extent, half_width],
            [length * 0.5, half_width],
            kind,
            0.,
            0.,
            self.color,
            self.space,
            self.transform,
            self.antialiasing,
            viewport,
        )
        .ok_or(Error::InvalidLineDraw)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BrushParameters {
    base: Parameters,
    origin_x: [f32; 4],
    axis_y: [f32; 4],
}
impl BrushParameters {
    fn new<T: PrimitiveData>(
        draw: T,
        viewport: [f32; 2],
        brush: &crate::Brush,
        graphics: &GraphicsContext,
    ) -> Result<Self, Error> {
        let base = draw.parameters(viewport)?;
        let (center, x, y) = draw.object_frame();
        let bx = f64::from(base.axis_y_min[2])
            .abs()
            .max(f64::from(base.axis_y_min[2]) + f64::from(base.span_style[0]));
        let by = f64::from(base.axis_y_min[3])
            .abs()
            .max(f64::from(base.axis_y_min[3]) + f64::from(base.span_style[1]));
        let extent = std::array::from_fn(|i| {
            f64::from(center[i]).abs() + f64::from(x[i]).abs() * bx + f64::from(y[i]).abs() * by
        });
        if extent
            .iter()
            .any(|v| !v.is_finite() || *v > f64::from(f32::MAX) * 0.125)
        {
            return Err(Error::InvalidBrushDraw);
        }
        brush.validate(graphics, extent, base.color)?;
        Ok(Self {
            base,
            origin_x: [center[0], center[1], x[0], x[1]],
            axis_y: [y[0], y[1], 0., 0.],
        })
    }
}

// One private encoder serves both public APIs. Convert through f64 so validation
// catches overflow, including the analytic fringe under rotation/shear/scaling.
#[allow(clippy::too_many_arguments)]
fn pack(
    center: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
    bounds: [f32; 2],
    geometry: [f32; 2],
    kind: f32,
    radius: f32,
    stroke_width: f32,
    color: [f32; 4],
    space: DrawSpace,
    t: Transform2D,
    aa: EdgeAntialiasing,
    viewport: [f32; 2],
) -> Option<Parameters> {
    let [a, b, c, d, tx, ty] = t.0.map(f64::from);
    let [vx, vy] = viewport.map(f64::from);
    if vx == 0. || vy == 0. {
        return Some(Parameters::zeroed());
    }
    let scale = match space {
        DrawSpace::Pixels => [1., 1.],
        DrawSpace::Normalized => [vx, vy],
    };
    let axis = |p: [f32; 2]| {
        [
            (a * f64::from(p[0]) + c * f64::from(p[1])) * scale[0],
            (b * f64::from(p[0]) + d * f64::from(p[1])) * scale[1],
        ]
    };
    let ax = axis(x);
    let ay = axis(y);
    let determinant = ax[0] * ay[1] - ax[1] * ay[0];
    if determinant == 0. {
        return Some(Parameters::zeroed());
    }
    let padding = if aa == EdgeAntialiasing::Coverage {
        [
            (ay[0].abs() + ay[1].abs()) / determinant.abs(),
            (ax[0].abs() + ax[1].abs()) / determinant.abs(),
        ]
    } else {
        [0., 0.]
    };
    let bx = f64::from(bounds[0]) + padding[0];
    let by = f64::from(bounds[1]) + padding[1];
    let origin = [
        (a * f64::from(center[0]) + c * f64::from(center[1]) + tx) * scale[0] / vx,
        (b * f64::from(center[0]) + d * f64::from(center[1]) + ty) * scale[1] / vy,
    ];
    let p = Parameters {
        origin_axis_x: [
            origin[0] as f32,
            origin[1] as f32,
            (ax[0] / vx) as f32,
            (ax[1] / vy) as f32,
        ],
        axis_y_min: [
            (ay[0] / vx) as f32,
            (ay[1] / vy) as f32,
            -bx as f32,
            -by as f32,
        ],
        span_style: [(bx * 2.) as f32, (by * 2.) as f32, kind, radius],
        geometry: [
            geometry[0],
            geometry[1],
            stroke_width,
            if aa == EdgeAntialiasing::Coverage {
                1.
            } else {
                0.
            },
        ],
        color,
        radii: [0.; 4],
    };
    if !bytemuck::cast_slice::<Parameters, f32>(&[p])
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    for sx in [-bx, bx] {
        for sy in [-by, by] {
            let point = [
                origin[0] + sx * ax[0] / vx + sy * ay[0] / vx,
                origin[1] + sx * ax[1] / vy + sy * ay[1] / vy,
            ];
            if point
                .iter()
                .any(|v| !(*v as f32).is_finite() || !(*v as f32 * 2.).is_finite())
            {
                return None;
            }
        }
    }
    Some(p)
}

#[derive(Debug)]
struct PrimitiveRenderer {
    graphics: GraphicsContext,
    material: Material,
    brush_material: Option<Material>,
    pipelines: std::collections::HashMap<
        (u64, wgpu::TextureFormat, u32, Option<wgpu::TextureFormat>),
        crate::pipeline_cache::CachedPipeline,
    >,
    parameters: Vec<Parameters>,
    brush_parameters: Vec<BrushParameters>,
    page_capacity: usize,
    brush_page_capacity: usize,
}
impl PrimitiveRenderer {
    fn new(g: &GraphicsContext, options: crate::PipelineOptions) -> Self {
        let shader = g
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("RXUI drawing solid primitives"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::clip::shader(g, include_str!("primitive.wgsl")).into(),
                ),
            });
        let layout=VertexLayout {stride:STRIDE,step_mode:wgpu::VertexStepMode::Instance,
            attributes:wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4,4=>Float32x4,5=>Float32x4].to_vec()};
        let layouts = [layout, crate::clip::layout(8)];
        Self {
            graphics: g.clone(),
            material: g.create_material(
                options.mesh(
                    MaterialOptions::new(&shader)
                        .bind_group_layouts(&crate::clip::layouts(
                            g,
                            None,
                            &g.rounded_clip_layout(),
                        ))
                        .vertex_layouts(&layouts)
                        .entry_points(
                            "vertex_main",
                            if options.writes_attachment() {
                                "fragment_covered"
                            } else {
                                "fragment_main"
                            },
                        ),
                ),
            ),
            pipelines: Default::default(),
            parameters: Vec::new(),
            brush_parameters: Vec::new(),
            brush_material: None,
            page_capacity: (g.device().limits().max_buffer_size / STRIDE).clamp(1, PAGE_INSTANCES)
                as usize,
            brush_page_capacity: (g.device().limits().max_buffer_size / BRUSH_STRIDE)
                .clamp(1, 64 * 1024 / BRUSH_STRIDE) as usize,
        }
    }
    fn pipeline(&mut self, format: &RenderFormat) -> Result<wgpu::RenderPipeline, Error> {
        self.pipeline_for(
            format.single_color()?,
            format.sample_count,
            format.depth_stencil,
            false,
        )
    }
    fn pipeline_for(
        &mut self,
        color: wgpu::TextureFormat,
        sample_count: u32,
        depth_stencil: Option<wgpu::TextureFormat>,
        brushed: bool,
    ) -> Result<wgpu::RenderPipeline, Error> {
        if brushed && self.brush_material.is_none() {
            let limits = self.graphics.device().limits();
            if limits.max_vertex_attributes < 11
                || u64::from(limits.max_vertex_buffer_array_stride) < BRUSH_STRIDE
                || limits.max_buffer_size < BRUSH_STRIDE
                || limits.max_inter_stage_shader_variables < 8
            {
                return Err(Error::UnsupportedBrushLimits);
            }
            let layout=VertexLayout {stride:BRUSH_STRIDE,step_mode:wgpu::VertexStepMode::Instance,attributes:wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4,4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4].to_vec()};
            self.brush_material = Some(crate::brush::material(
                &self.graphics,
                &self.material,
                &[layout, crate::clip::layout(8)],
                &crate::clip::shader(&self.graphics, include_str!("primitive.wgsl")),
                include_str!("primitive_brush.wgsl"),
            )?);
        }
        let material = if brushed {
            self.brush_material.as_ref().unwrap()
        } else {
            &self.material
        };
        // Built-in solid shading requires floating-point, blendable color, just as
        // built-in mesh shading does. Inert depth/stencil permits sharing a 3D pass.
        Ok(crate::pipelines::pipeline(
            &self.graphics,
            &mut self.pipelines,
            material,
            color,
            sample_count,
            depth_stencil,
            true,
        )
        .map_err(|error| match error {
            Error::UnsupportedMeshFormat { format } => Error::UnsupportedPrimitiveFormat { format },
            other => other,
        })?
        .clone())
    }
    fn bind<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
        brush: Option<&'draw crate::Brush>,
    ) -> Result<Session<'draw, 'frame>, Error> {
        if !pass.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        if let Some(brush) = brush {
            brush.validate_device(&self.graphics)?;
        }
        crate::pipelines::validate_aspects(pass, &self.material)?;
        let pipeline = self.pipeline_for(
            pass.single_color_format()?,
            pass.sample_count(),
            pass.depth_stencil_format(),
            brush.is_some(),
        )?;
        pass.apply_raster_state();
        pass.set_pipeline(&pipeline);
        Ok(Session {
            pass,
            pipeline,
            parameters: &mut self.parameters,
            dirty: false,
            clips_bound: false,
            brush,
            graphics: &self.graphics,
            brush_parameters: &mut self.brush_parameters,
            page_capacity: self.page_capacity,
            brush_page_capacity: self.brush_page_capacity,
        })
    }
}
#[derive(Debug)]
struct Session<'draw, 'frame> {
    pass: &'draw mut RenderPass<'frame>,
    pipeline: wgpu::RenderPipeline,
    parameters: &'draw mut Vec<Parameters>,
    dirty: bool,
    clips_bound: bool,
    brush: Option<&'draw crate::Brush>,
    graphics: &'draw GraphicsContext,
    brush_parameters: &'draw mut Vec<BrushParameters>,
    page_capacity: usize,
    brush_page_capacity: usize,
}
impl<'frame> Session<'_, 'frame> {
    fn restore(&mut self) {
        if self.dirty {
            self.pass.apply_raster_state();
            self.pass.set_pipeline(&self.pipeline);
            self.dirty = false;
        }
        // The session exclusively borrows the pass. Only pass() can expose clip,
        // viewport, or binding changes; it invalidates this state. Keep binding
        // lazy so empty/rejected draws never upload clips or replace app bindings.
        if !self.clips_bound {
            self.pass.bind_clip(1);
            self.pass.bind_rounded_clips();
            self.clips_bound = true;
        }
    }
    fn draw<T: PrimitiveData>(&mut self, draw: T) -> Result<(), Error> {
        if let Some(brush) = self.brush {
            let p = BrushParameters::new(draw, self.pass.viewport_size(), brush, self.graphics)?;
            self.restore();
            brush.bind(self.pass);
            record(
                self.pass,
                std::slice::from_ref(&p),
                self.brush_page_capacity,
            );
            return Ok(());
        }
        let p = draw.parameters(self.pass.viewport_size())?;
        self.restore();
        record(self.pass, std::slice::from_ref(&p), self.page_capacity);
        Ok(())
    }
    fn draw_many<T: PrimitiveData>(&mut self, draws: &[T]) -> Result<(), Error> {
        if let Some(brush) = self.brush {
            self.brush_parameters.clear();
            for &draw in draws {
                self.brush_parameters.push(BrushParameters::new(
                    draw,
                    self.pass.viewport_size(),
                    brush,
                    self.graphics,
                )?);
            }
            if !self.brush_parameters.is_empty() {
                self.restore();
                brush.bind(self.pass);
                record(self.pass, self.brush_parameters, self.brush_page_capacity);
            }
            return Ok(());
        }
        self.parameters.clear();
        for draw in draws {
            self.parameters
                .push(draw.parameters(self.pass.viewport_size())?);
        }
        if !self.parameters.is_empty() {
            self.restore();
            record(self.pass, self.parameters, self.page_capacity);
        }
        Ok(())
    }
    fn pass(&mut self) -> &mut RenderPass<'frame> {
        self.dirty = true;
        self.clips_bound = false;
        self.pass
    }
}
fn record<T: Pod>(pass: &mut RenderPass<'_>, parameters: &[T], capacity: usize) {
    let stride = std::mem::size_of::<T>() as u64;
    for chunk in parameters.chunks(capacity) {
        let first = pass.bind_instances(0, bytemuck::cast_slice(chunk), stride);
        pass.draw_instances(
            crate::pass::DrawGeometry::Vertices(0..6),
            first..first + chunk.len() as u32,
        );
    }
}

/// Independent renderer for filled/outlined rectangles, rounded rectangles, and ellipses.
///
/// Owns shaders, pipeline variants, and reusable CPU scratch, but no windows or
/// frames. Colors are linear straight RGBA; output uses premultiplied source-over.
/// Default shading ignores depth/stencil. [`Self::with_options`] configures explicit
/// tests/writes, including stencil masks controlled by the pass reference.
/// Shader edge coverage works without MSAA. No draw sorting or implicit batching
/// occurs. Prepare pipelines before rendering to avoid first-use pipeline creation.
#[derive(Debug)]
pub struct ShapeRenderer {
    inner: PrimitiveRenderer,
}
impl ShapeRenderer {
    /// Creates built-in shading with default pipeline settings.
    pub fn new(graphics: &GraphicsContext) -> Self {
        Self::with_options(graphics, crate::PipelineOptions::default())
    }
    /// Creates built-in shading with immutable blend/write/depth/stencil settings.
    /// Compatibility is checked during preparation or drawing, before any draw.
    pub fn with_options(graphics: &GraphicsContext, options: crate::PipelineOptions) -> Self {
        Self {
            inner: PrimitiveRenderer::new(graphics, options),
        }
    }
    /// Prepares a target's color/sample/depth-stencil variant without drawing.
    pub fn prepare(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.inner.pipeline(format)?;
        Ok(())
    }
    /// Prepares a surface variant, rejecting targets on another device.
    pub fn prepare_for_target(&mut self, target: &crate::RenderTarget<'_>) -> Result<(), Error> {
        if !target.graphics().same_device(&self.inner.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(&target.render_format())
    }
    /// Warms the shared brush shader for these attachments/MSAA without creating brushes.
    pub fn prepare_brush(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.inner.pipeline_for(
            format.single_color()?,
            format.sample_count,
            format.depth_stencil,
            true,
        )?;
        Ok(())
    }
    /// Records one primitive with a brush in original geometry coordinates.
    /// Draw color acts as straight RGBA tint; white preserves the brush. Owns bind group zero.
    pub fn draw_with_brush(
        &mut self,
        pass: &mut RenderPass<'_>,
        brush: &crate::Brush,
        draw: ShapeDraw,
    ) -> Result<(), Error> {
        self.bind_with_brush(pass, brush)?.draw(draw)
    }
    /// Validates the whole slice before recording ordered instances with one brush.
    /// Uploads 128 bytes per placement; stops stay retained. Empty slices are a no-op.
    pub fn draw_many_with_brush(
        &mut self,
        pass: &mut RenderPass<'_>,
        brush: &crate::Brush,
        draws: &[ShapeDraw],
    ) -> Result<(), Error> {
        if draws.is_empty() {
            return Ok(());
        }
        self.bind_with_brush(pass, brush)?.draw_many(draws)
    }
    /// Borrows the pass with one brush and pipeline selected for repeated draws.
    /// Pass access invalidates state; the next draw restores pipeline and brush bindings.
    pub fn bind_with_brush<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
        brush: &'draw crate::Brush,
    ) -> Result<ShapeDrawSession<'draw, 'frame>, Error> {
        Ok(ShapeDrawSession {
            inner: self.inner.bind(pass, Some(brush))?,
        })
    }
    /// Validates and records one primitive. Rejected operations record no draw.
    pub fn draw(&mut self, pass: &mut RenderPass<'_>, draw: ShapeDraw) -> Result<(), Error> {
        self.bind(pass)?.draw(draw)
    }
    /// Validates the whole slice, then instances consecutive primitives in input
    /// order, splitting at upload-page boundaries. Empty slices are a no-op.
    /// Validation failure records no draw from this slice. Storage is reused after warm-up.
    pub fn draw_many(
        &mut self,
        pass: &mut RenderPass<'_>,
        draws: &[ShapeDraw],
    ) -> Result<(), Error> {
        if draws.is_empty() {
            return Ok(());
        }
        self.bind(pass)?.draw_many(draws)
    }
    /// Selects the pipeline once for repeated drawing in an exclusive pass scope.
    /// Dynamic geometry still gets validated and uploaded through frame-owned pages.
    /// Dropping the scope leaves the pass open; it does not submit or present.
    pub fn bind<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
    ) -> Result<ShapeDrawSession<'draw, 'frame>, Error> {
        Ok(ShapeDrawSession {
            inner: self.inner.bind(pass, None)?,
        })
    }
}
/// A scoped shape pipeline with validated dynamic data and explicit pass access.
/// Created with [`ShapeRenderer::bind`]. Borrows renderer scratch and the pass.
#[derive(Debug)]
pub struct ShapeDrawSession<'draw, 'frame> {
    inner: Session<'draw, 'frame>,
}
impl<'frame> ShapeDrawSession<'_, 'frame> {
    /// Validates and records one primitive in the current viewport and clip.
    pub fn draw(&mut self, draw: ShapeDraw) -> Result<(), Error> {
        self.inner.draw(draw)
    }
    /// Validates the whole slice before recording ordered, page-sized instance batches.
    pub fn draw_many(&mut self, draws: &[ShapeDraw]) -> Result<(), Error> {
        self.inner.draw_many(draws)
    }
    /// Borrows the pass for clipping, bindings, other renderers, or raw commands.
    /// The next draw restores this scope's pipeline and wrapped raster state.
    pub fn pass(&mut self) -> &mut RenderPass<'frame> {
        self.inner.pass()
    }
}

/// Independent renderer for independent width-aware line segments.
///
/// Owns shaders, pipeline variants, and reusable CPU scratch, but no windows or
/// frames. Colors are linear straight RGBA; output uses premultiplied source-over.
/// Default shading ignores depth/stencil. [`Self::with_options`] configures explicit
/// tests/writes, including stencil masks controlled by the pass reference.
/// Shader edge coverage works without MSAA. No draw sorting or implicit batching
/// occurs. Prepare pipelines before rendering to avoid first-use pipeline creation.
#[derive(Debug)]
pub struct LineRenderer {
    inner: PrimitiveRenderer,
}
impl LineRenderer {
    /// Creates built-in shading with default pipeline settings.
    pub fn new(graphics: &GraphicsContext) -> Self {
        Self::with_options(graphics, crate::PipelineOptions::default())
    }
    /// Creates built-in shading with immutable blend/write/depth/stencil settings.
    /// Compatibility is checked during preparation or drawing, before any draw.
    pub fn with_options(graphics: &GraphicsContext, options: crate::PipelineOptions) -> Self {
        Self {
            inner: PrimitiveRenderer::new(graphics, options),
        }
    }
    /// Prepares a target's color/sample/depth-stencil variant without drawing.
    pub fn prepare(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.inner.pipeline(format)?;
        Ok(())
    }
    /// Prepares a surface variant, rejecting targets on another device.
    pub fn prepare_for_target(&mut self, target: &crate::RenderTarget<'_>) -> Result<(), Error> {
        if !target.graphics().same_device(&self.inner.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(&target.render_format())
    }
    /// Warms the shared brush shader for these attachments/MSAA without creating brushes.
    pub fn prepare_brush(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.inner.pipeline_for(
            format.single_color()?,
            format.sample_count,
            format.depth_stencil,
            true,
        )?;
        Ok(())
    }
    /// Records one primitive with a brush in original geometry coordinates.
    /// Draw color acts as straight RGBA tint; white preserves the brush. Owns bind group zero.
    pub fn draw_with_brush(
        &mut self,
        pass: &mut RenderPass<'_>,
        brush: &crate::Brush,
        draw: LineDraw,
    ) -> Result<(), Error> {
        self.bind_with_brush(pass, brush)?.draw(draw)
    }
    /// Validates the whole slice before recording ordered instances with one brush.
    /// Uploads 128 bytes per placement; stops stay retained. Empty slices are a no-op.
    pub fn draw_many_with_brush(
        &mut self,
        pass: &mut RenderPass<'_>,
        brush: &crate::Brush,
        draws: &[LineDraw],
    ) -> Result<(), Error> {
        if draws.is_empty() {
            return Ok(());
        }
        self.bind_with_brush(pass, brush)?.draw_many(draws)
    }
    /// Borrows the pass with one brush and pipeline selected for repeated draws.
    /// Pass access invalidates state; the next draw restores pipeline and brush bindings.
    pub fn bind_with_brush<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
        brush: &'draw crate::Brush,
    ) -> Result<LineDrawSession<'draw, 'frame>, Error> {
        Ok(LineDrawSession {
            inner: self.inner.bind(pass, Some(brush))?,
        })
    }
    /// Validates and records one primitive. Rejected operations record no draw.
    pub fn draw(&mut self, pass: &mut RenderPass<'_>, draw: LineDraw) -> Result<(), Error> {
        self.bind(pass)?.draw(draw)
    }
    /// Validates the whole slice, then instances consecutive primitives in input
    /// order, splitting at upload-page boundaries. Empty slices are a no-op.
    /// Validation failure records no draw from this slice. Storage is reused after warm-up.
    pub fn draw_many(
        &mut self,
        pass: &mut RenderPass<'_>,
        draws: &[LineDraw],
    ) -> Result<(), Error> {
        if draws.is_empty() {
            return Ok(());
        }
        self.bind(pass)?.draw_many(draws)
    }
    /// Selects the pipeline once for repeated drawing in an exclusive pass scope.
    /// Dynamic geometry still gets validated and uploaded through frame-owned pages.
    /// Dropping the scope leaves the pass open; it does not submit or present.
    pub fn bind<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
    ) -> Result<LineDrawSession<'draw, 'frame>, Error> {
        Ok(LineDrawSession {
            inner: self.inner.bind(pass, None)?,
        })
    }
}
/// A scoped line pipeline with validated dynamic data and explicit pass access.
/// Created with [`LineRenderer::bind`]. Borrows renderer scratch and the pass.
#[derive(Debug)]
pub struct LineDrawSession<'draw, 'frame> {
    inner: Session<'draw, 'frame>,
}
impl<'frame> LineDrawSession<'_, 'frame> {
    /// Validates and records one primitive in the current viewport and clip.
    pub fn draw(&mut self, draw: LineDraw) -> Result<(), Error> {
        self.inner.draw(draw)
    }
    /// Validates the whole slice before recording ordered, page-sized instance batches.
    pub fn draw_many(&mut self, draws: &[LineDraw]) -> Result<(), Error> {
        self.inner.draw_many(draws)
    }
    /// Borrows the pass for clipping, bindings, other renderers, or raw commands.
    /// The next draw restores this scope's pipeline and wrapped raster state.
    pub fn pass(&mut self) -> &mut RenderPass<'frame> {
        self.inner.pass()
    }
}

#[cfg(test)]
#[path = "primitive_tests.rs"]
mod tests;
