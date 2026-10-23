use crate::{
    Error, GraphicsContext, LineDraw, LineRenderer, Path, PathDraw, PathOptions, PathRenderer,
    PreparedPath, PreparedText, Rect, RenderFormat, RenderPass, ShapeDraw, ShapeRenderer, Stroke,
    TextDraw, TextLayout, TextPreparation, TextRenderError, TextRenderer, TextureBinding,
    TextureBindingOptions, TextureDraw, TextureRenderer, Transform2D,
};

/// Reusable 2D drawing resources composed from independent primitive/path/image/text renderers.
/// Optional polyline and marker renderers share application-owned point buffers;
/// [`Self::prepare_points`] checks and warms their vertex-storage pipelines separately.
///
/// Create once per device and retain across frames. [`Self::begin`] borrows an
/// application-owned pass; it never acquires a frame, clears attachments, submits,
/// or presents. Calls record immediately in caller order. There is no pending
/// command list and no implicit sorting/batching. Use explicit batch methods when
/// consecutive compatible draws can be instanced. The underlying renderers remain
/// independently usable, and custom GPU work fits through [`PaintSession::pass`].
///
/// Colors, coordinates, transforms, caps, coverage, and errors follow [`ShapeDraw`],
/// [`LineDraw`], [`PathDraw`], [`TextureDraw`], and [`TextDraw`]. Text shaping and rasterization
/// are explicit: prepare a [`TextLayout`] before [`Self::prepare_text`], retain the
/// returned [`PreparedText`], and reuse it across sessions. Prepare variants before
/// first use to avoid pipeline creation while recording. Paths are tessellated explicitly
/// through [`Self::prepare_path`], retaining geometry independently of draw color/transform.
/// After warm-up, a fixed batch workload reuses
/// GPU pages and CPU scratch; transformed batches retain capacity up to their peak.
///
/// ```no_run
/// use rxui_draw::{Painter, PreparedText, RenderPass, TextDraw, TextLayout,
///     TextRasterOptions, TextRenderError, Transform2D};
/// // Call when layout/content/DPI changes, before entering a painting session.
/// fn prepare(painter: &mut Painter, layout: &TextLayout, dpi: f32)
///     -> Result<PreparedText, TextRenderError>
/// {
///     painter.prepare_text(layout, TextRasterOptions::new().raster_scale(dpi))
/// }
/// fn paint(painter: &mut Painter, pass: &mut RenderPass<'_>, text: &PreparedText, dpi: f32)
///     -> Result<(), Box<dyn std::error::Error>>
/// {
///     let mut paint = painter.begin(pass)?;
///     // A shared geometry transform applies DPI once to every 2D renderer.
///     let mut logical = paint.transformed(Transform2D::scale(dpi, dpi))?;
///     let mut local = logical.transformed(Transform2D::translation(20., 30.))?;
///     local.draw_text(text, TextDraw::default().color([0.8, 0.9, 1., 1.]))?;
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct Painter {
    graphics: GraphicsContext,
    shapes: ShapeRenderer,
    lines: LineRenderer,
    paths: PathRenderer,
    polylines: crate::PolylineRenderer,
    markers: crate::MarkerRenderer,
    textures: TextureRenderer,
    text: TextRenderer,
    shape_scratch: Vec<ShapeDraw>,
    line_scratch: Vec<LineDraw>,
    image_scratch: Vec<TextureDraw>,
    path_scratch: Vec<PathDraw>,
}
impl Painter {
    /// Creates independent shape, line, path, polyline, marker, image, and text renderers.
    /// Pipeline caches start empty; point-rendering capabilities are checked on use.
    pub fn new(graphics: &GraphicsContext) -> Self {
        Self::with_options(graphics, crate::PipelineOptions::default())
    }
    /// Configures the same immutable pipeline policy for all owned renderers.
    /// Depth/stencil attachment allocation and dynamic references belong to the pass.
    /// Text uses default atlas budgets; replace [`Self::text`] to select other budgets,
    /// preserving its pipeline options when the same clipping policy is wanted.
    pub fn with_options(graphics: &GraphicsContext, options: crate::PipelineOptions) -> Self {
        Self {
            graphics: graphics.clone(),
            shapes: ShapeRenderer::with_options(graphics, options.clone()),
            lines: LineRenderer::with_options(graphics, options.clone()),
            paths: PathRenderer::with_options(graphics, options.clone()),
            polylines: crate::PolylineRenderer::with_options(graphics, options.clone()),
            markers: crate::MarkerRenderer::with_options(graphics, options.clone()),
            textures: TextureRenderer::with_options(graphics, options.clone()),
            text: TextRenderer::with_pipeline(graphics, options),
            shape_scratch: Vec::new(),
            line_scratch: Vec::new(),
            image_scratch: Vec::new(),
            path_scratch: Vec::new(),
        }
    }
    /// Prepares default primitive and text pipelines for this attachment format.
    /// Does not shape text, rasterize glyphs, or upload prepared text geometry.
    /// Image variants also depend on a source binding; use [`Self::prepare_image`].
    pub fn prepare(&mut self, format: &RenderFormat) -> Result<(), Error> {
        profiling::scope!("rxui_draw::Painter::prepare");
        {
            profiling::scope!("rxui_draw::prepare_shape_pipeline");
            self.shapes.prepare(format)?;
        }
        {
            profiling::scope!("rxui_draw::prepare_line_pipeline");
            self.lines.prepare(format)?;
        }
        {
            profiling::scope!("rxui_draw::prepare_path_pipeline");
            self.paths.prepare(format)?;
        }
        {
            profiling::scope!("rxui_draw::prepare_text_pipelines");
            self.text.prepare(format)
        }
    }
    /// Warms brush shading for shapes, lines, and paths in these attachments/MSAA.
    /// Brushes are created separately on GraphicsContext; this uploads no stops or geometry.
    pub fn prepare_brush(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.shapes.prepare_brush(format)?;
        self.lines.prepare_brush(format)?;
        self.paths.prepare_brush(format)
    }
    /// Warms optional dense polyline/marker shading and checks vertex-storage support.
    /// Ordinary prepare does not require point-rendering capabilities. Samples stay separate.
    pub fn prepare_points(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.polylines.prepare(format)?;
        self.markers.prepare(format)
    }
    /// Borrows GPU-expanded connected-line rendering over updateable point buffers.
    pub fn polylines(&mut self) -> &mut crate::PolylineRenderer {
        &mut self.polylines
    }
    /// Borrows GPU-expanded screen-pixel markers over updateable point buffers.
    pub fn markers(&mut self) -> &mut crate::MarkerRenderer {
        &mut self.markers
    }
    /// Prepares primitive and text pipelines for a surface, rejecting a foreign device.
    pub fn prepare_for_target(&mut self, target: &crate::RenderTarget<'_>) -> Result<(), Error> {
        if !target.graphics().same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(&target.render_format())
    }
    /// Prepares ordinary and transformed-retained image variants for this source and format.
    pub fn prepare_image(
        &mut self,
        image: &TextureBinding,
        format: &RenderFormat,
    ) -> Result<(), Error> {
        self.textures.prepare(image, format)?;
        self.textures.prepare_transformed(image, format)
    }
    /// Uploads immutable image placements once, outside an active painting session.
    /// Pixel placements retain the supplied viewport size; normalized placements adapt.
    /// Each placement keeps its UVs, tint and transform. Image source stays separate.
    pub fn prepare_images(
        &self,
        draws: &[TextureDraw],
        viewport: [f32; 2],
    ) -> Result<crate::PreparedTextureDraw, Error> {
        self.textures.prepare_draws(draws, viewport)
    }
    /// Creates a reusable snapshot image/sampler binding. Placement remains per draw.
    pub fn create_image_binding(
        &self,
        view: &wgpu::TextureView,
        options: TextureBindingOptions,
    ) -> Result<TextureBinding, Error> {
        self.textures.create_binding(view, options)
    }
    /// Creates a live premultiplied framebuffer binding which follows storage replacement.
    pub fn create_sampled_binding(
        &self,
        source: &crate::SampledColor,
    ) -> Result<TextureBinding, Error> {
        self.textures.create_sampled_binding(source)
    }
    /// Creates a live framebuffer binding with explicit alpha and filtering, for example
    /// nearest-filtered premultiplied layers composited at their native pixel size.
    pub fn create_sampled_binding_with_options(
        &self,
        source: &crate::SampledColor,
        options: TextureBindingOptions,
    ) -> Result<TextureBinding, Error> {
        self.textures
            .create_sampled_binding_with_options(source, options)
    }
    /// Borrows the underlying renderer for direct preparation, drawing, or scopes.
    pub fn shapes(&mut self) -> &mut ShapeRenderer {
        &mut self.shapes
    }
    /// Borrows the underlying renderer for direct preparation, drawing, or scopes.
    pub fn lines(&mut self) -> &mut LineRenderer {
        &mut self.lines
    }
    /// Borrows the independent path renderer for preparation, drawing, or scopes.
    pub fn paths(&mut self) -> &mut PathRenderer {
        &mut self.paths
    }
    /// Tessellates and uploads a retained fill/stroke outside an active session.
    /// Color, placement, and edge coverage remain per draw. Does not prepare an
    /// attachment pipeline. Retain the result until geometry/settings change.
    pub fn prepare_path(
        &mut self,
        path: &Path,
        options: PathOptions,
    ) -> Result<PreparedPath, Error> {
        self.paths.prepare_path(path, options)
    }
    /// Borrows the underlying renderer for custom samplers/materials, prepared data,
    /// live sources, direct drawing, or scopes outside an active painting session.
    pub fn textures(&mut self) -> &mut TextureRenderer {
        &mut self.textures
    }
    /// Borrows the text renderer for pipeline preparation, cache statistics, cache
    /// clearing, or direct drawing outside an active painting session. Prepared texts
    /// from any TextRenderer on this device are also accepted by the session.
    /// To configure atlas budgets, replace this renderer with
    /// [`TextRenderer::with_options`] before preparing text; retained resources keep
    /// their original allocation leases.
    pub fn text(&mut self) -> &mut TextRenderer {
        &mut self.text
    }
    /// Generates missing glyph images and uploads immutable text geometry outside painting.
    /// Pass `TextRasterOptions` for coverage or `MtsdfOptions` for scalable outline fill.
    /// The layout retains its fonts; Painter never borrows or owns a TextSystem.
    /// Raster scale controls image quality; geometry retains the layout's units. Retain the
    /// result until content, layout, or preparation settings change; placement/color/opacity
    /// remain per draw. Cache pressure and unsupported glyphs follow
    /// [`TextRenderer::prepare_text`]. This does not prepare an attachment pipeline.
    pub fn prepare_text(
        &mut self,
        layout: &TextLayout,
        preparation: impl Into<TextPreparation>,
    ) -> Result<PreparedText, TextRenderError> {
        self.text.prepare_text(layout, preparation)
    }
    /// Prepares layouts in input order with shared geometry uploads, outside painting.
    /// Results remain independently drawable. Settings, chunk size, ownership and
    /// failure behavior follow [`TextRenderer::prepare_texts`]. Accepts borrowed
    /// layouts and `Arc<TextLayout>` collections; no TextSystem is borrowed.
    pub fn prepare_texts<L: AsRef<TextLayout>>(
        &mut self,
        layouts: impl IntoIterator<Item = L>,
        preparation: impl Into<TextPreparation>,
    ) -> Result<Vec<PreparedText>, TextRenderError> {
        self.text.prepare_texts(layouts, preparation)
    }
    /// Starts immediate painting with identity transform on an existing color pass.
    /// Device/output checks happen here; individual draws validate geometry and resources.
    /// Dropping the session leaves the pass open with its current raster settings.
    pub fn begin<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
    ) -> Result<PaintSession<'draw, 'frame>, Error> {
        if !pass.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        pass.single_color_format()?;
        Ok(PaintSession {
            painter: self,
            pass,
            transform: Transform2D::IDENTITY,
            restore: None,
        })
    }
}

/// Immediate 2D painting into an exclusively borrowed application pass.
///
/// Geometry retains its own units. The session transform is applied after each
/// draw's transform, in those same units, before viewport conversion. Use pixels
/// plus an explicit scale for application logical coordinates; normalized draws
/// still interpret translations/widths in viewport fractions. [`Self::transformed`]
/// lends a child with additional local geometry transformation, preserving the
/// parent's transform without a mutable save/restore stack. [`Self::clipped`]
/// lends a child whose scissor is narrowed to a local rectangle and restored when
/// the child drops. Neither saves or restores the viewport or application bindings.
///
/// Pass access records in place with no flush required. Following paint calls
/// restore the state their renderer needs. Application groups and chosen wrapped
/// raster settings remain caller-controlled. Painter does not own stencil clipping
/// policy, layout, windows, text shaping, or a retained display list.
/// The exclusive pass borrow prevents overlapping direct use:
///
/// ```compile_fail
/// use rxui_draw::{Error, Painter, Rect, RenderPass};
/// fn overlap(painter: &mut Painter, pass: &mut RenderPass<'_>) -> Result<(), Error> {
///     let mut paint = painter.begin(pass)?;
///     pass.set_stencil_reference(1);
///     paint.fill_rect(Rect::new(0.,0.,10.,10.), [1.;4])?;
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct PaintSession<'draw, 'frame> {
    painter: &'draw mut Painter,
    pass: &'draw mut RenderPass<'frame>,
    transform: Transform2D,
    // Parent clip state, restored when a clipping child drops.
    restore: Option<SavedClip>,
}
#[derive(Clone, Debug)]
struct SavedClip {
    scissor: [u32; 4],
    rounded: Vec<crate::RoundedClip>,
}
impl Drop for PaintSession<'_, '_> {
    fn drop(&mut self) {
        if let Some(saved) = self.restore.take() {
            self.pass.restore_scissor(saved.scissor);
            // The saved clip was validated when it was set.
            let _ = self.pass.set_rounded_clips(&saved.rounded);
        }
    }
}
impl<'frame> PaintSession<'_, 'frame> {
    /// Draws retained connected samples with the session transform applied on the GPU.
    /// Width stays in screen pixels. No sample upload, scan, or CPU tessellation occurs.
    pub fn draw_polyline(
        &mut self,
        points: &crate::PointBuffer,
        mut draw: crate::PolylineDraw,
    ) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.polylines.draw(self.pass, points, draw)
    }
    /// Draws a retained sample range as screen-pixel markers. Radius stays fixed under zoom.
    /// Gaps are skipped; overlap order follows the logical sample sequence.
    pub fn draw_markers(
        &mut self,
        points: &crate::PointBuffer,
        mut draw: crate::MarkerDraw,
    ) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.markers.draw(self.pass, points, draw)
    }
    /// Draws a shape with a reusable brush in original geometry coordinates.
    /// Draw color acts as tint; the session transform moves geometry and brush together.
    pub fn draw_shape_with_brush(
        &mut self,
        brush: &crate::Brush,
        mut draw: ShapeDraw,
    ) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.shapes.draw_with_brush(self.pass, brush, draw)
    }
    /// Instances an ordered shape batch with one brush, validating it before drawing.
    /// Transformed batches reuse Painter-owned CPU scratch.
    pub fn draw_shapes_with_brush(
        &mut self,
        brush: &crate::Brush,
        draws: &[ShapeDraw],
    ) -> Result<(), Error> {
        if self.transform == Transform2D::IDENTITY {
            return self
                .painter
                .shapes
                .draw_many_with_brush(self.pass, brush, draws);
        }
        self.painter.shape_scratch.clear();
        self.painter
            .shape_scratch
            .extend(draws.iter().map(|d| ShapeDraw {
                transform: d.transform.then(self.transform),
                ..*d
            }));
        self.painter
            .shapes
            .draw_many_with_brush(self.pass, brush, &self.painter.shape_scratch)
    }
    /// Draws an independent line with a reusable brush in its original endpoint coordinates.
    /// Color acts as tint. Brush coordinates are not automatically measured along the line.
    pub fn draw_line_with_brush(
        &mut self,
        brush: &crate::Brush,
        mut draw: LineDraw,
    ) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.lines.draw_with_brush(self.pass, brush, draw)
    }
    /// Instances an ordered line batch with one brush, preserving per-line geometry and tint.
    pub fn draw_lines_with_brush(
        &mut self,
        brush: &crate::Brush,
        draws: &[LineDraw],
    ) -> Result<(), Error> {
        if self.transform == Transform2D::IDENTITY {
            return self
                .painter
                .lines
                .draw_many_with_brush(self.pass, brush, draws);
        }
        self.painter.line_scratch.clear();
        self.painter
            .line_scratch
            .extend(draws.iter().map(|d| LineDraw {
                transform: d.transform.then(self.transform),
                ..*d
            }));
        self.painter
            .lines
            .draw_many_with_brush(self.pass, brush, &self.painter.line_scratch)
    }
    /// Draws retained path geometry with a reusable brush, without tessellation or stop uploads.
    /// Color acts as tint; original path coordinates define brush placement before transforms.
    pub fn draw_path_with_brush(
        &mut self,
        path: &PreparedPath,
        brush: &crate::Brush,
        mut draw: PathDraw,
    ) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter
            .paths
            .draw_with_brush(self.pass, path, brush, draw)
    }
    /// Instances ordered placements of one retained path with one brush.
    /// Geometry, stops, and pipelines stay reusable; transformed batches reuse CPU scratch.
    pub fn draw_paths_with_brush(
        &mut self,
        path: &PreparedPath,
        brush: &crate::Brush,
        draws: &[PathDraw],
    ) -> Result<(), Error> {
        if self.transform == Transform2D::IDENTITY {
            return self
                .painter
                .paths
                .draw_many_with_brush(self.pass, path, brush, draws);
        }
        self.painter.path_scratch.clear();
        self.painter
            .path_scratch
            .extend(draws.iter().map(|d| PathDraw {
                transform: d.transform.then(self.transform),
                ..*d
            }));
        self.painter
            .paths
            .draw_many_with_brush(self.pass, path, brush, &self.painter.path_scratch)
    }
    /// Borrows the raw wgpu pass for application-provided pipelines and draws.
    /// Flushes pending UI draws. Subsequent UI drawing restores its tracked state.
    /// Custom shaders must implement rounded clipping themselves; the active
    /// rectangular scissor is already configured. Coordinates are physical pixels.
    pub fn wgpu(&mut self) -> &mut wgpu::RenderPass<'frame> {
        self.pass.as_wgpu()
    }

    /// Returns the transform applied after each draw's own transform.
    pub fn transform(&self) -> Transform2D {
        self.transform
    }
    /// Lends a child with `local` applied before the parent's transform. The parent
    /// becomes usable again when the child drops, with its transform unchanged.
    /// Nonfinite or overflowing composition rejects the child without changing state.
    pub fn transformed(
        &mut self,
        local: impl Into<Transform2D>,
    ) -> Result<PaintSession<'_, 'frame>, Error> {
        let local = local.into();
        let transform = local.then(self.transform);
        if !local.valid() || !transform.valid() {
            return Err(Error::InvalidTransform2D);
        }
        Ok(PaintSession {
            painter: self.painter,
            pass: self.pass,
            transform,
            restore: None,
        })
    }
    /// Lends a child whose drawing is limited to `rect`, in this session's local
    /// pixel units. The rectangle is mapped through the session transform; the
    /// scissor becomes the pixel-aligned bounds of the result, intersected with the
    /// pass's current scissor and the attachment. Rotation or shear clips to the
    /// axis-aligned bounds, and fractional edges round outward to whole pixels;
    /// [`Self::clipped_rounded`] clips exactly. A rectangle entirely outside the
    /// current clip gives an empty scissor, so the child draws nothing.
    ///
    /// The parent's scissor and rounded clip are restored when the child drops,
    /// including any the child set through [`Self::pass`]. Nested children intersect
    /// their parents. The scissor applies to every renderer drawing into the pass,
    /// including custom work.
    pub fn clipped(&mut self, rect: Rect) -> Result<PaintSession<'_, 'frame>, Error> {
        let scissor = self.clip_scissor(rect)?;
        self.clip_child(scissor, None)
    }
    /// Lends a child clipped to a rounded rectangle in this session's local pixel
    /// units, with anti-aliased edges that follow the session transform exactly,
    /// including rotation. The scissor is narrowed as in [`Self::clipped`], and the
    /// [`crate::RoundedClip`] intersection is evaluated by shapes, lines, paths,
    /// text, images, polylines and markers. Mesh/custom shaders use the explicit
    /// [`RenderPass::bind_rounded_clips`] interface or application stencil masks.
    /// Every nested rounded boundary remains active under its own transform.
    /// Zero radii give an exact anti-aliased rectangle. Dropping the child restores
    /// the full parent intersection. Exceeding the device's
    /// [`GraphicsContext::rounded_clip_capacity`] returns
    /// [`Error::ClipStackFull`] without changing the scissor or clip stack.
    pub fn clipped_rounded(
        &mut self,
        rect: Rect,
        radii: crate::CornerRadii,
    ) -> Result<PaintSession<'_, 'frame>, Error> {
        let clip = crate::RoundedClip::new(rect, radii).transform(self.transform);
        clip.validate()?;
        let scissor = self.clip_scissor(rect)?;
        self.clip_child(scissor, Some(clip))
    }
    fn clip_child(
        &mut self,
        scissor: [u32; 4],
        rounded: Option<crate::RoundedClip>,
    ) -> Result<PaintSession<'_, 'frame>, Error> {
        let saved = SavedClip {
            scissor: self.pass.scissor_rect(),
            rounded: self.pass.rounded_clips().to_vec(),
        };
        if let Some(clip) = rounded {
            self.pass.push_rounded_clip(clip)?;
        }
        self.pass.restore_scissor(scissor);
        Ok(PaintSession {
            painter: self.painter,
            pass: self.pass,
            transform: self.transform,
            restore: Some(saved),
        })
    }
    fn clip_scissor(&self, rect: Rect) -> Result<[u32; 4], Error> {
        let bounds = self.transform.transform_bounds(rect);
        if !rect.valid()
            || !bounds.valid()
            || !bounds.right().is_finite()
            || !bounds.bottom().is_finite()
        {
            return Err(Error::InvalidClip);
        }
        Ok(self.pass.scissor_within(bounds))
    }
    /// Records a filled rectangle in pixels, using linear straight RGBA.
    #[inline]
    pub fn fill_rect(&mut self, rect: Rect, color: [f32; 4]) -> Result<(), Error> {
        self.draw_shape(ShapeDraw::rect(rect, color))
    }
    /// Records a uniformly rounded rectangle; the radius is clamped by its renderer.
    #[inline]
    pub fn fill_rounded_rect(
        &mut self,
        rect: Rect,
        radius: f32,
        color: [f32; 4],
    ) -> Result<(), Error> {
        self.draw_shape(ShapeDraw::rounded_rect(rect, radius, color))
    }
    /// Records an ellipse inscribed in the pixel rectangle.
    #[inline]
    pub fn fill_ellipse(&mut self, rect: Rect, color: [f32; 4]) -> Result<(), Error> {
        self.draw_shape(ShapeDraw::ellipse(rect, color))
    }
    /// Records a sharp-corner rectangle outline in pixels. Width transforms with geometry.
    #[inline]
    pub fn stroke_rect(
        &mut self,
        rect: Rect,
        stroke: Stroke,
        color: [f32; 4],
    ) -> Result<(), Error> {
        self.draw_shape(ShapeDraw::rect(rect, color).stroke(stroke))
    }
    /// Records a rounded rectangle outline with an offset, uniformly rounded boundary.
    #[inline]
    pub fn stroke_rounded_rect(
        &mut self,
        rect: Rect,
        radius: f32,
        stroke: Stroke,
        color: [f32; 4],
    ) -> Result<(), Error> {
        self.draw_shape(ShapeDraw::rounded_rect(rect, radius, color).stroke(stroke))
    }
    /// Records an ellipse outline using a distance offset of the original curve.
    #[inline]
    pub fn stroke_ellipse(
        &mut self,
        rect: Rect,
        stroke: Stroke,
        color: [f32; 4],
    ) -> Result<(), Error> {
        self.draw_shape(ShapeDraw::ellipse(rect, color).stroke(stroke))
    }
    /// Records the blurred shadow of a rounded rectangle element at `rect`, in pixels.
    /// Draw it before the element. See [`ShapeDraw::box_shadow`] for spread and offset;
    /// batches of shadows and other shapes can share one [`Self::draw_shapes`] call.
    #[inline]
    pub fn draw_box_shadow(
        &mut self,
        rect: Rect,
        radii: crate::CornerRadii,
        shadow: crate::BoxShadow,
    ) -> Result<(), Error> {
        self.draw_shape(ShapeDraw::box_shadow(rect, radii, shadow))
    }
    /// Records explicit primitive geometry/space/coverage and its per-draw transform.
    #[inline]
    pub fn draw_shape(&mut self, mut draw: ShapeDraw) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.shapes.draw(self.pass, draw)
    }
    /// Validates a whole slice before recording ordered primitive instance batches.
    /// Transformed batches reuse Painter-owned CPU scratch; no sorting occurs.
    pub fn draw_shapes(&mut self, draws: &[ShapeDraw]) -> Result<(), Error> {
        if self.transform == Transform2D::IDENTITY {
            return self.painter.shapes.draw_many(self.pass, draws);
        }
        self.painter.shape_scratch.clear();
        self.painter
            .shape_scratch
            .extend(draws.iter().map(|d| ShapeDraw {
                transform: d.transform.then(self.transform),
                ..*d
            }));
        self.painter
            .shapes
            .draw_many(self.pass, &self.painter.shape_scratch)
    }
    /// Records an independent segment with explicit width/caps/coverage and transform.
    #[inline]
    pub fn draw_line(&mut self, mut draw: LineDraw) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.lines.draw(self.pass, draw)
    }
    /// Validates a whole slice before recording ordered line instance batches.
    pub fn draw_lines(&mut self, draws: &[LineDraw]) -> Result<(), Error> {
        if self.transform == Transform2D::IDENTITY {
            return self.painter.lines.draw_many(self.pass, draws);
        }
        self.painter.line_scratch.clear();
        self.painter
            .line_scratch
            .extend(draws.iter().map(|d| LineDraw {
                transform: d.transform.then(self.transform),
                ..*d
            }));
        self.painter
            .lines
            .draw_many(self.pass, &self.painter.line_scratch)
    }
    /// Records an image with explicit placement, UVs, tint, and transform.
    /// The binding is reusable and can track a live framebuffer source.
    #[inline]
    pub fn draw_image(
        &mut self,
        image: &TextureBinding,
        mut draw: TextureDraw,
    ) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.textures.draw(self.pass, image, draw)
    }
    /// Draws retained image placements with this session's transform and clipping.
    /// Identity sessions need no parameter uploads. Transformed sessions upload
    /// only 48 bytes, preserving retained geometry and immediate ordering.
    /// Pixel placements must match the current viewport, as in direct texture drawing.
    pub fn draw_prepared_images(
        &mut self,
        image: &TextureBinding,
        draws: &crate::PreparedTextureDraw,
    ) -> Result<(), Error> {
        self.painter
            .textures
            .draw_prepared_transformed(self.pass, image, draws, self.transform)
    }
    /// Records retained text immediately in caller order with shapes, images, lines,
    /// and custom pass work. No shaping, rasterization, atlas writes, or glyph geometry
    /// uploads occur here. The origin and glyph geometry retain layout units.
    /// The session transform follows the draw transform, consistently with other
    /// 2D geometry. Raster density controls image quality and never doubles DPI scaling. Color tints coverage glyphs; intrinsic
    /// color glyphs retain their RGB. Color alpha and opacity affect both kinds.
    /// Invalid draws record no text commands and leave the session usable.
    #[inline]
    pub fn draw_text(&mut self, text: &PreparedText, mut draw: TextDraw) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.text.draw(self.pass, text, draw)
    }
    /// Draws prepared vector geometry; the session transform follows its draw transform.
    /// No tessellation or geometry upload occurs during painting.
    pub fn draw_path(&mut self, path: &PreparedPath, mut draw: PathDraw) -> Result<(), Error> {
        if self.transform != Transform2D::IDENTITY {
            draw.transform = draw.transform.then(self.transform);
        }
        self.painter.paths.draw(self.pass, path, draw)
    }
    /// Instances ordered placements of one retained path, validating the whole slice.
    /// The identity session forwards directly; transformed sessions reuse CPU scratch.
    pub fn draw_paths(&mut self, path: &PreparedPath, draws: &[PathDraw]) -> Result<(), Error> {
        if self.transform == Transform2D::IDENTITY {
            return self.painter.paths.draw_many(self.pass, path, draws);
        }
        self.painter.path_scratch.clear();
        self.painter.path_scratch.extend(
            draws
                .iter()
                .map(|d| d.transform(d.transform.then(self.transform))),
        );
        self.painter
            .paths
            .draw_many(self.pass, path, &self.painter.path_scratch)
    }
    /// Validates a whole slice, then instances ordered placements of one image.
    pub fn draw_images(
        &mut self,
        image: &TextureBinding,
        draws: &[TextureDraw],
    ) -> Result<(), Error> {
        if self.transform == Transform2D::IDENTITY {
            return self.painter.textures.draw_many(self.pass, image, draws);
        }
        self.painter.image_scratch.clear();
        self.painter.image_scratch.extend(
            draws
                .iter()
                .map(|d| d.transform(d.transform.then(self.transform))),
        );
        self.painter
            .textures
            .draw_many(self.pass, image, &self.painter.image_scratch)
    }
    /// Borrows the existing pass for wrapped clipping/viewport changes, application
    /// bindings, other renderers, or raw commands. Geometry transforms affect only
    /// Painter draws; this access forwards the pass without applying a transform.
    pub fn pass(&mut self) -> &mut RenderPass<'frame> {
        self.pass
    }
}

#[cfg(test)]
#[path = "painter_tests.rs"]
mod tests;
