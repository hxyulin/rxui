use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt as _;

use crate::{Error, GraphicsContext, Transform2D};

/// One gradient stop, using linear RGB and straight alpha.
///
/// Positions must be finite, in `0..=1`, and supplied in nondecreasing order.
/// Equal positions create a hard transition: the last stop at that position wins.
/// RGB may contain finite HDR values; alpha must be in `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Position along the gradient's zero-to-one interval.
    pub position: f32,
    /// Linear, straight RGBA; interpolation happens after premultiplication.
    pub color: [f32; 4],
}
impl GradientStop {
    /// Creates a stop; [`GraphicsContext::create_brush`] validates the complete sequence.
    pub const fn new(position: f32, color: [f32; 4]) -> Self {
        Self { position, color }
    }
}

/// How a gradient extends beyond its zero-to-one interval.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientSpread {
    /// Extend endpoint colors, the default.
    #[default]
    Clamp,
    /// Repeat every interval; exact integer boundaries select position zero.
    Repeat,
    /// Alternate forward and backward intervals, with period two.
    Reflect,
}

/// Geometry and stops used to create an immutable GPU [`Brush`].
#[derive(Clone, Copy, Debug)]
pub enum BrushKind<'a> {
    /// Constant linear, straight RGBA.
    Solid([f32; 4]),
    /// Project onto the nonzero segment from `start` (zero) to `end` (one).
    Linear {
        /// Start in brush coordinates.
        start: [f32; 2],
        /// End in brush coordinates, different from start.
        end: [f32; 2],
        /// At least one ordered stop. One stop produces constant color.
        stops: &'a [GradientStop],
    },
    /// Centered circular gradient, from radius zero to `radius` (one).
    /// An affine brush transform can turn the circle into an ellipse.
    Radial {
        /// Center in brush coordinates.
        center: [f32; 2],
        /// Finite, strictly positive radius.
        radius: f32,
        /// At least one ordered stop.
        stops: &'a [GradientStop],
    },
}

/// Borrowed brush creation settings, copied/uploaded by [`GraphicsContext::create_brush`].
///
/// Coordinates use geometry's original units before its draw transform and
/// viewport conversion. `transform` maps brush coordinates into geometry units;
/// drawing applies its inverse. It must be finite and invertible for gradients.
/// There is no automatic bounding-box normalization or window DPI conversion.
/// Shared brushes follow each draw's geometry transform, including Painter scopes.
/// Stop interpolation is linear in premultiplied RGBA; it avoids hidden RGB in
/// transparent stops bleeding into visible color. Missing zero/one stops extend
/// the first/last color within the interval. Stops are neither sorted nor clamped.
#[derive(Clone, Copy, Debug)]
pub struct BrushOptions<'a> {
    /// Constant color or gradient geometry/stops.
    pub kind: BrushKind<'a>,
    /// Brush-to-geometry transform; identity by default. Ignored for solid brushes.
    pub transform: Transform2D,
    /// Gradient extension; clamp by default. Ignored for solid brushes.
    pub spread: GradientSpread,
}
impl<'a> BrushOptions<'a> {
    /// Creates constant shading. The ordinary draw color-only API remains the cheaper route.
    pub const fn solid(color: [f32; 4]) -> Self {
        Self::new(BrushKind::Solid(color))
    }
    /// Creates a linear gradient projected onto the supplied segment.
    pub const fn linear(start: [f32; 2], end: [f32; 2], stops: &'a [GradientStop]) -> Self {
        Self::new(BrushKind::Linear { start, end, stops })
    }
    /// Creates a centered radial gradient; an affine transform can make it elliptical.
    pub const fn radial(center: [f32; 2], radius: f32, stops: &'a [GradientStop]) -> Self {
        Self::new(BrushKind::Radial {
            center,
            radius,
            stops,
        })
    }
    const fn new(kind: BrushKind<'a>) -> Self {
        Self {
            kind,
            transform: Transform2D::IDENTITY,
            spread: GradientSpread::Clamp,
        }
    }
    /// Maps brush coordinates into the geometry's original coordinate system.
    pub fn transform(mut self, transform: impl Into<Transform2D>) -> Self {
        self.transform = transform.into();
        self
    }
    /// Selects clamp, repeat, or alternating reflection outside the interval.
    pub const fn spread(mut self, spread: GradientSpread) -> Self {
        self.spread = spread;
        self
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniform {
    axes: [f32; 4],
    translation: [f32; 4],
    style: [u32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Stop {
    position: [f32; 4],
    color: [f32; 4],
}
#[derive(Debug)]
struct Storage {
    group: wgpu::BindGroup,
    // Keep handles for diagnostics; the bind group also retains their GPU storage.
    uniform: wgpu::Buffer,
    stops: wgpu::Buffer,
}

/// Immutable device-bound shading shared by paths, shapes, and independent lines.
///
/// Create on [`GraphicsContext`], then use a renderer's `draw_with_brush`,
/// `draw_many_with_brush`, or `bind_with_brush`. Geometry and brush resources are
/// independent: changing a brush does not tessellate/upload path geometry. Clones
/// share immutable GPU handles. Brushes from any context/renderer on the same
/// device are accepted. Draw color multiplies the brush as a straight RGBA tint;
/// use white to preserve it. Coverage multiplies the final premultiplied result.
/// Brush draws own bind group zero; custom pass work must establish its bindings.
///
/// Creation uploads 48 uniform bytes and 32 bytes per stop. Drawing uploads no
/// stops. Gradient sampling uses binary search over exact stops, rather than a
/// sampled color ramp. There is no fixed stop-count cap beyond device buffer
/// limits. Fragment storage-buffer support is required and checked at creation.
/// CPU descriptions, stop slices, and renderer lifetimes need not outlive a brush.
///
/// ```no_run
/// use rxui_draw::{Brush, BrushOptions, Error, GradientStop, GraphicsContext};
/// fn gradient(graphics: &GraphicsContext) -> Result<Brush, Error> {
///     graphics.create_brush(BrushOptions::linear([0., 0.], [100., 0.], &[
///         GradientStop::new(0., [0.1, 0.3, 1., 1.]),
///         GradientStop::new(1., [1., 0.2, 0.1, 0.5]),
///     ]))
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Brush {
    graphics: GraphicsContext,
    storage: Arc<Storage>,
    uniform: Uniform,
    max_color: [f64; 3],
    count: u32,
}
impl Brush {
    /// Number of retained stops; solid brushes retain one constant stop.
    pub fn stop_count(&self) -> u32 {
        self.count
    }
    /// Retained uniform/stop buffer bytes, excluding driver metadata.
    pub fn buffer_bytes(&self) -> u64 {
        self.storage.uniform.size() + self.storage.stops.size()
    }
    pub(crate) fn create(
        graphics: &GraphicsContext,
        options: BrushOptions<'_>,
    ) -> Result<Self, Error> {
        check_support(graphics)?;
        let count = match options.kind {
            BrushKind::Solid(_) => 1,
            BrushKind::Linear { stops, .. } | BrushKind::Radial { stops, .. } => stops.len(),
        };
        let limits = graphics.device().limits();
        if count > u32::MAX as usize
            || (count as u64).saturating_mul(32)
                > limits
                    .max_buffer_size
                    .min(limits.max_storage_buffer_binding_size)
        {
            return Err(Error::BrushTooLarge);
        }
        let (uniform, stops) = encode(options)?;
        let count = count as u32;
        let bytes = bytemuck::cast_slice(&stops);
        let uniform_buffer =
            graphics
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("RXUI drawing brush parameters"),
                    contents: bytemuck::bytes_of(&uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
        let stop_buffer = graphics
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("RXUI drawing gradient stops"),
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let group = graphics
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("RXUI drawing brush"),
                layout: &layout(graphics.device()),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: stop_buffer.as_entire_binding(),
                    },
                ],
            });
        let max_color = std::array::from_fn(|i| {
            stops
                .iter()
                .map(|s| f64::from(s.color[i]).abs())
                .fold(0., f64::max)
        });
        Ok(Self {
            graphics: graphics.clone(),
            storage: Arc::new(Storage {
                group,
                uniform: uniform_buffer,
                stops: stop_buffer,
            }),
            uniform,
            max_color,
            count,
        })
    }
    pub(crate) fn validate(
        &self,
        graphics: &GraphicsContext,
        extent: [f64; 2],
        tint: [f32; 4],
    ) -> Result<(), Error> {
        self.validate_device(graphics)?;
        if !crate::drawing::color_valid(tint) {
            return Err(Error::InvalidBrushDraw);
        }
        for (i, max) in self.max_color.iter().enumerate() {
            if max * f64::from(tint[i]).abs() * f64::from(tint[3]) > f64::from(f32::MAX) * 0.5 {
                return Err(Error::InvalidBrushDraw);
            }
        }
        if self.uniform.style[0] != 0 {
            for i in 0..2 {
                let bound = f64::from(self.uniform.axes[i]).abs() * extent[0]
                    + f64::from(self.uniform.axes[i + 2]).abs() * extent[1]
                    + f64::from(self.uniform.translation[i]).abs();
                if !bound.is_finite() || bound > f64::from(f32::MAX) * 0.125 {
                    return Err(Error::InvalidBrushDraw);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn validate_device(&self, graphics: &GraphicsContext) -> Result<(), Error> {
        if !self.graphics.same_device(graphics) {
            return Err(Error::DeviceMismatch);
        }
        Ok(())
    }
    pub(crate) fn bind(&self, pass: &mut crate::RenderPass<'_>) {
        pass.set_bind_group(0, &self.storage.group, &[]);
    }
}

fn encode(options: BrushOptions<'_>) -> Result<(Uniform, Vec<Stop>), Error> {
    let solid;
    let (kind, stops, base) = match options.kind {
        BrushKind::Solid(color) => {
            solid = [GradientStop::new(0., color)];
            (0, &solid[..], [0.; 6])
        }
        BrushKind::Linear { start, end, stops } => {
            if !start.into_iter().chain(end).all(f32::is_finite) {
                return Err(Error::InvalidBrush);
            }
            let [x, y] = start.map(f64::from);
            let [dx, dy] = [f64::from(end[0]) - x, f64::from(end[1]) - y];
            let length2 = dx * dx + dy * dy;
            if length2 == 0. {
                return Err(Error::InvalidBrush);
            }
            let [a, c] = [dx / length2, dy / length2];
            (1, stops, [a, 0., c, 0., -(a * x + c * y), 0.])
        }
        BrushKind::Radial {
            center,
            radius,
            stops,
        } => {
            if !center.into_iter().all(f32::is_finite) || !radius.is_finite() || radius <= 0. {
                return Err(Error::InvalidBrush);
            }
            let r = f64::from(radius);
            (
                2,
                stops,
                [
                    1. / r,
                    0.,
                    0.,
                    1. / r,
                    -f64::from(center[0]) / r,
                    -f64::from(center[1]) / r,
                ],
            )
        }
    };
    if stops.is_empty()
        || stops.iter().enumerate().any(|(i, s)| {
            !s.position.is_finite()
                || !(0. ..=1.).contains(&s.position)
                || !crate::drawing::color_valid(s.color)
                || (i > 0 && stops[i - 1].position > s.position)
        })
    {
        return Err(Error::InvalidBrush);
    }
    let mapping = if kind == 0 {
        base
    } else {
        let [a, b, c, d, x, y] = options.transform.0.map(f64::from);
        let det = a * d - b * c;
        if !options.transform.valid() || det == 0. {
            return Err(Error::InvalidBrush);
        }
        let inverse = [
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * y - d * x) / det,
            (b * x - a * y) / det,
        ];
        let [e, f, g, h, u, v] = base;
        let [a, b, c, d, x, y] = inverse;
        [
            e * a + g * b,
            f * a + h * b,
            e * c + g * d,
            f * c + h * d,
            e * x + g * y + u,
            f * x + h * y + v,
        ]
    };
    if mapping
        .iter()
        .any(|v| !v.is_finite() || v.abs() > f64::from(f32::MAX) * 0.125)
    {
        return Err(Error::InvalidBrush);
    }
    let [a, b, c, d, x, y] = mapping.map(|v| v as f32);
    let uniform = Uniform {
        axes: [a, b, c, d],
        translation: [x, y, 0., 0.],
        style: [
            kind,
            match options.spread {
                GradientSpread::Clamp => 0,
                GradientSpread::Repeat => 1,
                GradientSpread::Reflect => 2,
            },
            0,
            0,
        ],
    };
    let stops = stops
        .iter()
        .map(|s| Stop {
            position: [s.position, 0., 0., 0.],
            color: [
                s.color[0] * s.color[3],
                s.color[1] * s.color[3],
                s.color[2] * s.color[3],
                s.color[3],
            ],
        })
        .collect();
    Ok((uniform, stops))
}

pub(crate) fn check_support(graphics: &GraphicsContext) -> Result<(), Error> {
    let l = graphics.device().limits();
    if !graphics
        .adapter()
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::FRAGMENT_STORAGE)
        || l.max_bind_groups < 1
        || l.max_bindings_per_bind_group < 2
        || l.max_uniform_buffers_per_shader_stage < 1
        || l.max_storage_buffers_per_shader_stage < 1
        || l.max_uniform_buffer_binding_size < 48
        || l.max_storage_buffer_binding_size < 32
        || l.max_buffer_size < 48
    {
        return Err(Error::UnsupportedBrushLimits);
    }
    Ok(())
}
pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("RXUI drawing brush layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(48),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(32),
                },
                count: None,
            },
        ],
    })
}

// Reuse the exact geometry/coverage shader functions, with separate brush entry
// points and layouts. The ordinary solid shader and instance records stay intact.
pub(crate) fn shader(base: &str, entry_points: &str) -> String {
    format!("{base}\n{}\n{entry_points}", include_str!("brush.wgsl"))
}

pub(crate) fn material(
    graphics: &GraphicsContext,
    original: &crate::Material,
    layouts: &[crate::VertexLayout],
    base: &str,
    entries: &str,
) -> Result<crate::Material, Error> {
    check_support(graphics)?;
    let shader = graphics
        .device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("RXUI drawing brush shading"),
            source: wgpu::ShaderSource::Wgsl(shader(base, entries).into()),
        });
    let options = crate::PipelineOptions {
        blend: original.blend,
        write_mask: original.write_mask,
        depth_stencil: original.depth_stencil.clone(),
    };
    Ok(graphics.create_material(
        options.mesh(
            crate::MaterialOptions::new(&shader)
                .vertex_layouts(layouts)
                .bind_group_layouts(&crate::clip::layouts(
                    graphics,
                    Some(&layout(graphics.device())),
                    &graphics.rounded_clip_layout(),
                ))
                .entry_points(
                    "vertex_brush",
                    if options.writes_attachment() {
                        "fragment_brush_covered"
                    } else {
                        "fragment_brush"
                    },
                ),
        ),
    ))
}

#[cfg(test)]
mod tests;
