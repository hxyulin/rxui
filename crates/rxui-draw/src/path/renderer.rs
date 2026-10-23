use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};

use super::{Path, PathDraw, PathOptions, geometry};
use crate::{
    DrawSpace, EdgeAntialiasing, Error, GraphicsContext, Material, MaterialOptions,
    PipelineOptions, Rect, RenderFormat, RenderPass, RenderTarget, VertexLayout,
    geometry_pool::{Geometry, GeometryPool},
};

// Vertices followed by indices in one buffer.
#[derive(Debug)]
struct Storage {
    geometry: Geometry,
    index_offset: u64,
    index_end: u64,
}

/// Immutable device-bound fill or stroke geometry, shared by cheap clones.
///
/// Keep it across frames and change [`PathDraw`] color/transform without
/// tessellating or uploading vertices/indices again. Prepared paths from other
/// renderers on the same device are accepted. Storage is released when the last
/// owner and recorded GPU use finish; there is no global path cache. Geometry up to
/// 64 KiB goes back to its renderer's bounded idle pool instead, so re-preparing
/// changing paths reuses completed buffers.
/// Preparation retains coverage-fringe topology as well as interior triangles.
/// No attachment format, viewport size, or MSAA count is baked into this resource.
#[derive(Clone, Debug)]
pub struct PreparedPath {
    graphics: GraphicsContext,
    storage: Option<Arc<Storage>>,
    bounds: Option<Rect>,
    vertices: u32,
    indices: u32,
    interior_indices: u32,
    coordinate_extent: [f64; 2],
}
impl PreparedPath {
    /// Whether this resource contains no drawable triangles.
    pub fn is_empty(&self) -> bool {
        self.storage.is_none()
    }
    /// Bounds of the prepared fill/stroke in path units, excluding the screen-space fringe.
    /// Empty resources return None.
    pub fn bounds(&self) -> Option<Rect> {
        self.bounds
    }
    /// Number of retained vertices, including coverage-fringe vertices.
    pub fn vertex_count(&self) -> u32 {
        self.vertices
    }
    /// Number of retained indices, including coverage-fringe triangles.
    pub fn index_count(&self) -> u32 {
        self.indices
    }
    /// Vertex and index bytes of the retained geometry; empty resources retain no
    /// buffers. Pooled storage of up to 64 KiB rounds its allocation up to a power of two.
    pub fn geometry_bytes(&self) -> u64 {
        u64::from(self.vertices) * 32 + u64::from(self.indices) * 4
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Parameters {
    axes: [f32; 4],
    translation_viewport: [f32; 4],
    color: [f32; 4],
    style: [f32; 4],
}
impl Parameters {
    fn new(path: &PreparedPath, draw: PathDraw, viewport: [f32; 2]) -> Result<Self, Error> {
        if !draw.transform.valid() || !crate::drawing::color_valid(draw.color) {
            return Err(Error::InvalidPathDraw);
        }
        let [a, b, c, d, tx, ty] = draw.transform.0.map(f64::from);
        let [vx, vy] = viewport.map(f64::from);
        if vx == 0. || vy == 0. {
            return Ok(Self {
                translation_viewport: [0., 0., 1., 1.],
                ..Self::zeroed()
            });
        }
        let [sx, sy] = match draw.space {
            DrawSpace::Pixels => [1., 1.],
            DrawSpace::Normalized => [vx, vy],
        };
        let axes = [a * sx, b * sy, c * sx, d * sy];
        let translation = [tx * sx, ty * sy];
        let determinant = axes[0] * axes[3] - axes[1] * axes[2];
        if axes.iter().any(|v| v.abs() > f64::from(f32::MAX) * 0.25) {
            return Err(Error::InvalidPathDraw);
        }
        if path.bounds.is_some() {
            let [x, y] = path.coordinate_extent;
            // Bound intermediate shader arithmetic too, rather than allowing
            // cancellation to conceal overflow in a transformed corner.
            for (axis, v) in [(0, vx), (1, vy)] {
                let extent =
                    axes[axis].abs() * x + axes[axis + 2].abs() * y + translation[axis].abs() + 8.;
                if extent > f64::from(f32::MAX) * 0.125 || extent / v > f64::from(f32::MAX) * 0.125
                {
                    return Err(Error::InvalidPathDraw);
                }
            }
        }
        let p = Self {
            axes: axes.map(|v| v as f32),
            translation_viewport: [
                translation[0] as f32,
                translation[1] as f32,
                viewport[0],
                viewport[1],
            ],
            color: draw.color,
            style: [
                if determinant != 0. && draw.antialiasing == EdgeAntialiasing::Coverage {
                    1.
                } else {
                    0.
                },
                determinant.signum() as f32,
                0.,
                0.,
            ],
        };
        if !bytemuck::cast_slice::<Self, f32>(&[p])
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(Error::InvalidPathDraw);
        }
        Ok(p)
    }
}

/// Independent renderer for retained vector fills and connected strokes.
///
/// Preparation tessellates curves with Lyon, unions overlapping stroke triangles,
/// builds coverage fringes, and uploads immutable geometry synchronously. Drawing
/// does none of those operations: it uploads one 64-byte color/transform record
/// per placement through reusable frame pages. Pipeline variants are cached by
/// attachments and sample count. No automatic sorting or path cache is introduced.
///
/// Defaults use premultiplied source-over with inert depth/stencil. Coverage uses
/// a centered one-pixel screen-space band, with corner expansion bounded to four pixels.
/// It is approximate at narrow features, acute corners, and overlapping fringes;
/// [`EdgeAntialiasing::None`] draws only interior triangles and can use target MSAA.
/// Both modes preserve filled holes and avoid internal triangulation seams.
///
/// ```no_run
/// use rxui_draw::{Error, GraphicsContext, Path, PathDraw, PathOptions, PathRenderer,
///     PathStroke, PreparedPath, RenderPass, Transform2D};
/// fn prepare(graphics: &GraphicsContext, renderer: &mut PathRenderer)
///     -> Result<PreparedPath, Error>
/// {
///     let mut builder = Path::builder();
///     builder.move_to([0., 0.]).line_to([40., 0.]).line_to([20., 30.]).close();
///     renderer.prepare_path(&builder.build()?, PathOptions::new().stroke(PathStroke::new(2.)))
/// }
/// fn draw(renderer: &mut PathRenderer, pass: &mut RenderPass<'_>, path: &PreparedPath)
///     -> Result<(), Error>
/// {
///     renderer.draw(pass, path, PathDraw::new([0.2, 0.6, 1., 1.])
///         .transform(Transform2D::translation(20., 30.)))
/// }
/// ```
#[derive(Debug)]
pub struct PathRenderer {
    graphics: GraphicsContext,
    material: Material,
    brush_material: Option<Material>,
    pipelines: crate::pipelines::Pipelines,
    tessellators: geometry::Tessellators,
    geometry_pool: Arc<Mutex<GeometryPool>>,
    parameters: Vec<Parameters>,
    page_capacity: usize,
}
impl PathRenderer {
    /// Creates built-in path shading and empty pipeline/preparation caches.
    pub fn new(graphics: &GraphicsContext) -> Self {
        Self::with_options(graphics, PipelineOptions::default())
    }
    /// Configures immutable blending, writes, and depth/stencil policy.
    /// Dynamic stencil reference and attachment allocation remain pass-owned.
    pub fn with_options(graphics: &GraphicsContext, options: PipelineOptions) -> Self {
        let shader = graphics
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("RXUI drawing paths"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::clip::shader(graphics, include_str!("path.wgsl")).into(),
                ),
            });
        let layouts = [
            VertexLayout {
                stride: 32,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes:
                    wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x2,3=>Float32]
                        .to_vec(),
            },
            VertexLayout {
                stride: 64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes:
                    wgpu::vertex_attr_array![4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4]
                        .to_vec(),
            },
            crate::clip::layout(8),
        ];
        let material = graphics.create_material(
            options.mesh(
                MaterialOptions::new(&shader)
                    .bind_group_layouts(&crate::clip::layouts(
                        graphics,
                        None,
                        &graphics.rounded_clip_layout(),
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
        );
        Self {
            graphics: graphics.clone(),
            material,
            brush_material: None,
            pipelines: Default::default(),
            tessellators: Default::default(),
            geometry_pool: Default::default(),
            parameters: Vec::new(),
            page_capacity: (graphics.device().limits().max_buffer_size / 64).clamp(1, 1024)
                as usize,
        }
    }
    /// Tessellates a fill/stroke and uploads immutable geometry before recording.
    /// Invalid settings/tessellation/size errors allocate no GPU geometry buffers.
    /// Repeated calls prepare independent resources; retain the result explicitly.
    pub fn prepare_path(
        &mut self,
        path: &Path,
        options: PathOptions,
    ) -> Result<PreparedPath, Error> {
        profiling::scope!("rxui_draw::prepare_path");
        let geometry = self.tessellators.prepare(path, options)?;
        let vertices = u32::try_from(geometry.vertices.len()).map_err(|_| Error::PathTooLarge)?;
        let indices = u32::try_from(geometry.indices.len()).map_err(|_| Error::PathTooLarge)?;
        let vertex_bytes = bytemuck::cast_slice(&geometry.vertices);
        let index_bytes = bytemuck::cast_slice(&geometry.indices);
        let limit = self.graphics.device().limits().max_buffer_size;
        if vertex_bytes.len() as u64 > limit || index_bytes.len() as u64 > limit {
            self.tessellators.recycle(geometry);
            return Err(Error::PathTooLarge);
        }
        let storage = if indices == 0 {
            None
        } else {
            let index_offset = vertex_bytes.len() as u64;
            let index_end = index_offset + index_bytes.len() as u64;
            if index_end > limit {
                self.tessellators.recycle(geometry);
                return Err(Error::PathTooLarge);
            }
            let (buffer, _) = GeometryPool::upload(
                &self.geometry_pool,
                &self.graphics,
                "RXUI drawing prepared path",
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX,
                &[vertex_bytes, index_bytes],
            );
            Some(Arc::new(Storage {
                geometry: buffer,
                index_offset,
                index_end,
            }))
        };
        let prepared = PreparedPath {
            graphics: self.graphics.clone(),
            storage,
            bounds: geometry.bounds,
            vertices,
            indices,
            interior_indices: geometry.interior_indices,
            coordinate_extent: geometry.coordinate_extent,
        };
        self.tessellators.recycle(geometry);
        Ok(prepared)
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
        samples: u32,
        depth_stencil: Option<wgpu::TextureFormat>,
        brushed: bool,
    ) -> Result<wgpu::RenderPipeline, Error> {
        if brushed && self.brush_material.is_none() {
            if self
                .graphics
                .device()
                .limits()
                .max_inter_stage_shader_variables
                < 5
            {
                return Err(Error::UnsupportedBrushLimits);
            }
            self.brush_material = Some(crate::brush::material(
                &self.graphics,
                &self.material,
                &self.material.vertex_layouts,
                &crate::clip::shader(&self.graphics, include_str!("path.wgsl")),
                include_str!("brush.wgsl"),
            )?);
        }
        let material = if brushed {
            self.brush_material.as_ref().unwrap()
        } else {
            &self.material
        };
        Ok(crate::pipelines::pipeline(
            &self.graphics,
            &mut self.pipelines,
            material,
            color,
            samples,
            depth_stencil,
            true,
        )
        .map_err(|e| match e {
            Error::UnsupportedMeshFormat { format } => Error::UnsupportedPrimitiveFormat { format },
            other => other,
        })?
        .clone())
    }
    /// Prepares an attachment/MSAA pipeline; does not tessellate or upload paths.
    pub fn prepare(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.pipeline(format)?;
        Ok(())
    }
    /// Warms brush shading for these attachments/MSAA, without uploading brush or path data.
    /// All brushes share this variant; ordinary solid drawing keeps its own cheaper pipeline.
    pub fn prepare_brush(&mut self, format: &RenderFormat) -> Result<(), Error> {
        self.pipeline_for(
            format.single_color()?,
            format.sample_count,
            format.depth_stencil,
            true,
        )?;
        Ok(())
    }
    /// Prepares a surface variant, rejecting another device.
    pub fn prepare_for_target(&mut self, target: &RenderTarget<'_>) -> Result<(), Error> {
        if !target.graphics().same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(&target.render_format())
    }
    /// Records a retained path in place, restoring wrapped raster/pipeline/geometry state.
    /// Rejected draws record no geometry commands. Empty/singular paths draw no area.
    pub fn draw(
        &mut self,
        pass: &mut RenderPass<'_>,
        path: &PreparedPath,
        draw: PathDraw,
    ) -> Result<(), Error> {
        self.bind(pass)?.draw(path, draw)
    }
    /// Validates all placements, then instances one retained path in input order.
    /// Splits at reusable 64 KiB parameter pages. Invalid batches record no draws.
    /// Coverage None and Coverage can be mixed without creating pipeline variants.
    pub fn draw_many(
        &mut self,
        pass: &mut RenderPass<'_>,
        path: &PreparedPath,
        draws: &[PathDraw],
    ) -> Result<(), Error> {
        if draws.is_empty() {
            return Ok(());
        }
        self.bind(pass)?.draw_many(path, draws)
    }
    /// Draws retained geometry with an immutable brush in original path coordinates.
    /// Draw color multiplies it as a straight RGBA tint; white preserves the brush.
    /// This owns bind group zero. Rejected draws record no geometry commands.
    pub fn draw_with_brush(
        &mut self,
        pass: &mut RenderPass<'_>,
        path: &PreparedPath,
        brush: &crate::Brush,
        draw: PathDraw,
    ) -> Result<(), Error> {
        self.bind_with_brush(pass, brush)?.draw(path, draw)
    }
    /// Instances ordered placements of one path with one brush. Validates the whole batch.
    /// Only placement parameters are uploaded; stops and geometry stay retained.
    pub fn draw_many_with_brush(
        &mut self,
        pass: &mut RenderPass<'_>,
        path: &PreparedPath,
        brush: &crate::Brush,
        draws: &[PathDraw],
    ) -> Result<(), Error> {
        if draws.is_empty() {
            return Ok(());
        }
        self.bind_with_brush(pass, brush)?.draw_many(path, draws)
    }
    /// Selects one pipeline for repeated retained path draws in an exclusive pass scope.
    /// Different prepared paths can share the scope. Pass access invalidates its
    /// state, which the next path draw restores. Dropping leaves the pass open.
    pub fn bind<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
    ) -> Result<PathDrawSession<'draw, 'frame>, Error> {
        self.bind_internal(pass, None)
    }
    /// Selects brush shading and one reusable brush for repeated path draws.
    /// Different paths can share it. Pass access restores pipeline/bindings on the next draw.
    pub fn bind_with_brush<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
        brush: &'draw crate::Brush,
    ) -> Result<PathDrawSession<'draw, 'frame>, Error> {
        brush.validate_device(&self.graphics)?;
        self.bind_internal(pass, Some(brush))
    }
    fn bind_internal<'draw, 'frame>(
        &'draw mut self,
        pass: &'draw mut RenderPass<'frame>,
        brush: Option<&'draw crate::Brush>,
    ) -> Result<PathDrawSession<'draw, 'frame>, Error> {
        if !pass.same_device(&self.graphics) {
            return Err(Error::DeviceMismatch);
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
        Ok(PathDrawSession {
            graphics: &self.graphics,
            pass,
            pipeline,
            parameters: &mut self.parameters,
            dirty: false,
            page_capacity: self.page_capacity,
            brush,
        })
    }
}

/// Borrowed path drawing scope with one prepared attachment pipeline.
#[derive(Debug)]
pub struct PathDrawSession<'draw, 'frame> {
    graphics: &'draw GraphicsContext,
    pass: &'draw mut RenderPass<'frame>,
    pipeline: wgpu::RenderPipeline,
    parameters: &'draw mut Vec<Parameters>,
    dirty: bool,
    page_capacity: usize,
    brush: Option<&'draw crate::Brush>,
}
impl<'frame> PathDrawSession<'_, 'frame> {
    fn validate_device(&self, path: &PreparedPath) -> Result<(), Error> {
        if !path.graphics.same_device(self.graphics) {
            return Err(Error::DeviceMismatch);
        }
        Ok(())
    }
    fn restore(&mut self) {
        if self.dirty {
            self.pass.apply_raster_state();
            self.pass.set_pipeline(&self.pipeline);
            self.dirty = false;
        }
    }
    /// Validates and draws one retained path with a 64-byte parameter upload.
    pub fn draw(&mut self, path: &PreparedPath, draw: PathDraw) -> Result<(), Error> {
        self.validate_device(path)?;
        let p = Parameters::new(path, draw, self.pass.viewport_size())?;
        if let Some(brush) = self.brush {
            brush.validate(self.graphics, path.coordinate_extent, draw.color)?;
        }
        self.restore();
        if let Some(brush) = self.brush {
            brush.bind(self.pass);
        }
        record(
            self.pass,
            path,
            std::slice::from_ref(&p),
            draw.antialiasing == EdgeAntialiasing::None,
            self.page_capacity,
        );
        Ok(())
    }
    /// Validates all placements before recording ordered instance batches.
    /// CPU parameter scratch retains capacity up to the largest batch seen.
    pub fn draw_many(&mut self, path: &PreparedPath, draws: &[PathDraw]) -> Result<(), Error> {
        self.validate_device(path)?;
        self.parameters.clear();
        for &draw in draws {
            let p = Parameters::new(path, draw, self.pass.viewport_size())?;
            if let Some(brush) = self.brush {
                brush.validate(self.graphics, path.coordinate_extent, draw.color)?;
            }
            self.parameters.push(p);
        }
        self.restore();
        if let Some(brush) = self.brush {
            brush.bind(self.pass);
        }
        record(
            self.pass,
            path,
            self.parameters,
            draws
                .iter()
                .all(|d| d.antialiasing == EdgeAntialiasing::None),
            self.page_capacity,
        );
        Ok(())
    }
    /// Borrows the pass for clipping, another renderer, or raw commands.
    /// The next draw restores this scope's pipeline and wrapped raster state.
    pub fn pass(&mut self) -> &mut RenderPass<'frame> {
        self.dirty = true;
        self.pass
    }
}
fn record(
    pass: &mut RenderPass<'_>,
    path: &PreparedPath,
    parameters: &[Parameters],
    hard: bool,
    capacity: usize,
) {
    let Some(storage) = &path.storage else {
        return;
    };
    if parameters.is_empty() {
        return;
    }
    // The lease keeps a recycled buffer out of reuse until this recording completes.
    pass.retain_resource_ref(&storage.geometry.lease);
    let buffer = &storage.geometry.buffer;
    pass.set_vertex_buffer(0, buffer, 0..storage.index_offset);
    pass.set_index_buffer(
        buffer,
        storage.index_offset..storage.index_end,
        wgpu::IndexFormat::Uint32,
    );
    let count = if hard {
        path.interior_indices
    } else {
        path.indices
    };
    pass.bind_clip(2);
    pass.bind_rounded_clips();
    for chunk in parameters.chunks(capacity) {
        let first = pass.bind_instances(1, bytemuck::cast_slice(chunk), 64);
        pass.draw_instances(
            crate::pass::DrawGeometry::Indexed(0..count),
            first..first + chunk.len() as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FramebufferOptions, Transform2D, framebuffer::tests::pixels};

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn warmed_paths_reuse_storage_and_recordings_keep_dropped_geometry_valid() {
        pollster::block_on(async {
            let g = GraphicsContext::headless().await.unwrap();
            let mut renderer = PathRenderer::new(&g);
            let mut builder = Path::builder();
            builder
                .move_to([0., 0.])
                .line_to([8., 0.])
                .line_to([0., 8.])
                .close();
            let source = builder.build().unwrap();
            let path = renderer.prepare_path(&source, PathOptions::new()).unwrap();
            let storage = path.storage.as_ref().unwrap().clone();
            let mut target = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ),
                )
                .unwrap();
            let pipeline = renderer.pipeline(&target.render_format()).unwrap();
            let draws: Vec<_> = (0..1300)
                .map(|i| {
                    PathDraw::new([0.5, 0.3, 0.8, 0.5]).transform(Transform2D::translation(
                        (i % 8) as f32 * 7.,
                        ((i / 8) % 8) as f32 * 7.,
                    ))
                })
                .collect();
            pixels(&g, &mut target, |f| {
                renderer
                    .draw_many(&mut f.render_pass().begin().unwrap(), &path, &draws)
                    .unwrap()
            });
            let capacity = renderer.parameters.capacity();
            let buffers: Vec<_> = g
                .upload_pool
                .lock()
                .unwrap()
                .iter()
                .map(|(b, _)| b.clone())
                .collect();
            for _ in 0..3 {
                pixels(&g, &mut target, |f| {
                    renderer
                        .draw_many(&mut f.render_pass().begin().unwrap(), &path, &draws)
                        .unwrap()
                });
                assert_eq!(renderer.parameters.capacity(), capacity);
                assert_eq!(renderer.pipelines.len(), 1);
                assert_eq!(renderer.pipelines.values().next().unwrap(), &pipeline);
                assert_eq!(
                    path.storage.as_ref().unwrap().geometry.buffer,
                    storage.geometry.buffer
                );
                let pool = g.upload_pool.lock().unwrap();
                assert!(pool.iter().all(|(b, _)| buffers.contains(b)));
                assert_eq!(
                    pool.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
                    draws.len() * 64
                );
            }
            drop(storage);
            let weak = Arc::downgrade(path.storage.as_ref().unwrap());
            let mut frame = target.begin_frame().unwrap();
            renderer
                .draw(
                    &mut frame.render_pass().begin().unwrap(),
                    &path,
                    PathDraw::default(),
                )
                .unwrap();
            drop(path);
            assert!(weak.upgrade().is_none());
            drop(frame);
            assert!(weak.upgrade().is_none());
            let submitted = renderer.prepare_path(&source, PathOptions::new()).unwrap();
            let weak = Arc::downgrade(submitted.storage.as_ref().unwrap());
            let bytes = pixels(&g, &mut target, |frame| {
                renderer
                    .draw(
                        &mut frame.render_pass().begin().unwrap(),
                        &submitted,
                        PathDraw::default(),
                    )
                    .unwrap();
                drop(submitted);
                assert!(weak.upgrade().is_none());
            });
            assert_eq!(&bytes[(2 * 64 + 2) * 4..(2 * 64 + 3) * 4], &[255; 4]);
            assert!(weak.upgrade().is_none());
            // Dropping a renderer releases its CPU scratch, pipeline cache and idle
            // geometry pool; recordings keep only the buffers they draw.
        });
    }

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn reprepared_paths_reuse_geometry_only_after_recorded_use_completes() {
        pollster::block_on(async {
            let g = GraphicsContext::headless().await.unwrap();
            let mut renderer = PathRenderer::new(&g);
            let square = |size: f32| {
                let mut builder = Path::builder();
                builder
                    .move_to([0., 0.])
                    .line_to([size, 0.])
                    .line_to([size, size])
                    .line_to([0., size])
                    .close();
                builder.build().unwrap()
            };
            let mut target = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ),
                )
                .unwrap();
            renderer.prepare(&target.render_format()).unwrap();
            let buffer = |p: &PreparedPath| p.storage.as_ref().unwrap().geometry.buffer.clone();
            let draw = PathDraw::new([1.; 4]).antialiasing(EdgeAntialiasing::None);
            let small = renderer
                .prepare_path(&square(8.), PathOptions::new())
                .unwrap();
            let first = buffer(&small);
            let bytes = pixels(&g, &mut target, |f| {
                renderer
                    .draw(&mut f.render_pass().begin().unwrap(), &small, draw)
                    .unwrap();
                // The recording still uses the dropped path's buffer, so the
                // next preparation must not overwrite it before submission.
                drop(small);
                let large = renderer
                    .prepare_path(&square(40.), PathOptions::new())
                    .unwrap();
                assert_ne!(buffer(&large), first);
            });
            assert_eq!(&bytes[(4 * 64 + 4) * 4..(4 * 64 + 5) * 4], &[255; 4]);
            assert_eq!(&bytes[(20 * 64 + 20) * 4..(20 * 64 + 21) * 4], &[0; 4]);
            // After completion the idle buffer is reused.
            g.device()
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            let again = renderer
                .prepare_path(&square(8.), PathOptions::new())
                .unwrap();
            assert_eq!(buffer(&again), first);
            let bytes = pixels(&g, &mut target, |f| {
                renderer
                    .draw(&mut f.render_pass().begin().unwrap(), &again, draw)
                    .unwrap();
            });
            assert_eq!(&bytes[(4 * 64 + 4) * 4..(4 * 64 + 5) * 4], &[255; 4]);
        });
    }
}
