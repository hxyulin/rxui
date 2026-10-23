//! Anti-aliased rounded-rectangle clipping evaluated by built-in 2D shading.

use crate::{CornerRadii, Error, Rect, Transform2D};
use bytemuck::{Pod, Zeroable};

/// A rounded-rectangle clip applied by built-in 2D shading in a [`crate::RenderPass`].
///
/// `rect` and `radii` use clip-local units; `transform` maps them to physical pixels
/// relative to the viewport's top-left, like a pixel-space draw transform. Any
/// invertible affine transform is supported, so rotated and scaled clips stay exact.
/// Coverage is filtered over about one pixel at the boundary, independently of MSAA,
/// and multiplies the output of shapes, lines, paths, text and built-in image shading.
/// Nested clips intersect, including their transformed rounded corners. Built-in
/// polylines and markers use the same coverage. Meshes and custom shaders opt in
/// with [`crate::GraphicsContext::rounded_clip_shader`], [`crate::GraphicsContext::rounded_clip_layout`] and
/// [`crate::RenderPass::bind_rounded_clips`], or use application-owned stencil masks.
/// [`crate::PaintSession::clipped_rounded`] also intersects the scissor and restores
/// the entire parent stack. The device exposes a checked
/// [`crate::GraphicsContext::rounded_clip_capacity`], at most [`MAX_ROUNDED_CLIPS`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedClip {
    /// Clip rectangle in clip-local units.
    pub rect: Rect,
    /// Corner radii in clip-local units, fitted to the rectangle like shape radii.
    pub radii: CornerRadii,
    /// Clip-local units to viewport-relative physical pixels.
    pub transform: Transform2D,
}
impl RoundedClip {
    /// A clip in viewport-relative pixels with an identity transform.
    pub const fn new(rect: Rect, radii: CornerRadii) -> Self {
        Self {
            rect,
            radii,
            transform: Transform2D::IDENTITY,
        }
    }
    /// Selects the transform from clip-local units to viewport-relative pixels.
    pub fn transform(mut self, transform: impl Into<Transform2D>) -> Self {
        self.transform = transform.into();
        self
    }
    /// Checks finite geometry, nonnegative extents/radii and an invertible transform.
    pub fn validate(&self) -> Result<(), Error> {
        if !self.rect.valid() || !self.radii.valid() || self.transform.invert().is_none() {
            return Err(Error::InvalidClip);
        }
        Ok(())
    }
    // Maps viewport-normalized positions to clip-local units centered on the rectangle.
    pub(crate) fn record(&self, viewport: [f32; 2]) -> ClipRecord {
        let inverse = self.transform.invert().unwrap_or(Transform2D::IDENTITY);
        let [a, b, c, d, x, y] = inverse.0;
        let [w, h] = viewport;
        let Rect {
            x: left,
            y: top,
            width,
            height,
        } = self.rect;
        ClipRecord {
            axes: [a * w, b * w, c * h, d * h],
            offset_half: [
                x - (left + width * 0.5),
                y - (top + height * 0.5),
                width * 0.5,
                height * 0.5,
            ],
            radii: self.radii.fitted(width, height),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ClipRecord {
    axes: [f32; 4],
    offset_half: [f32; 4],
    radii: [f32; 4],
}
// A negative half size disables clipping in the shader.
pub(crate) const DISABLED: ClipRecord = ClipRecord {
    axes: [0.; 4],
    offset_half: [0., 0., -1., -1.],
    radii: [0.; 4],
};
pub(crate) const SIZE: u64 = std::mem::size_of::<ClipRecord>() as u64;

/// One per-draw record at stride zero, at three consecutive shader locations.
pub(crate) fn layout(first: u32) -> crate::VertexLayout {
    crate::VertexLayout {
        stride: 0,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: (0..3)
            .map(|i| wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: i * 16,
                shader_location: first + i as u32,
            })
            .collect(),
    }
}

/// Prepends the shared clip functions to a built-in shader.
pub(crate) fn shader(graphics: &crate::GraphicsContext, base: &str) -> String {
    format!(
        "{}\n{}\n{base}",
        include_str!("clip.wgsl"),
        graphics.rounded_clip_shader()
    )
}

// Built-ins share the capability policy, including shaders that also need a
// fragment uniform in group zero. Never leave a placeholder group one when disabled.
pub(crate) fn layouts<'a>(
    graphics: &crate::GraphicsContext,
    first: Option<&'a wgpu::BindGroupLayout>,
    clip: &'a wgpu::BindGroupLayout,
) -> Vec<Option<&'a wgpu::BindGroupLayout>> {
    if graphics.rounded_clip_capacity() > 0 {
        vec![first, Some(clip)]
    } else {
        first.into_iter().map(Some).collect()
    }
}

/// Upper bound on simultaneous analytic rounded clips. The enabled device limits
/// may lower [`crate::GraphicsContext::rounded_clip_capacity`]. Exceeding that
/// capacity returns [`Error::ClipStackFull`] without changing pass state.
pub const MAX_ROUNDED_CLIPS: usize = 64;

pub(crate) fn stack_size(capacity: usize) -> u64 {
    16 + SIZE * capacity as u64
}

pub(crate) fn stack_bytes(clips: &[RoundedClip], viewport: [f32; 6], capacity: usize) -> Vec<u8> {
    let mut bytes = vec![0; stack_size(capacity) as usize];
    let header = [viewport[0], viewport[1], clips.len() as f32, 0.];
    bytes[..16].copy_from_slice(bytemuck::bytes_of(&header));
    for (chunk, clip) in bytes[16..]
        .as_chunks_mut::<{ SIZE as usize }>()
        .0
        .iter_mut()
        .zip(clips)
    {
        chunk.copy_from_slice(bytemuck::bytes_of(&clip.record([1., 1.])));
    }
    bytes
}

impl crate::GraphicsContext {
    /// Maximum simultaneous rounded clips on this device, capped at
    /// [`MAX_ROUNDED_CLIPS`]. Smaller application-selected buffer/uniform limits
    /// reduce the capacity. Fewer than two bind groups or two fragment uniform
    /// buffers disables clipping (one uniform is reserved for brushes).
    /// Zero means analytic clipping is unavailable; unclipped
    /// drawing still works. Excess clips return [`Error::ClipStackFull`].
    /// Computed once from the imported device limits; context clones preserve it.
    pub fn rounded_clip_capacity(&self) -> usize {
        self.rounded_clip_capacity
    }

    /// WGSL for custom fragment shaders, sized for this device's enabled limits.
    /// Pair with [`Self::rounded_clip_layout`] at group one, binding zero, and
    /// [`crate::RenderPass::bind_rounded_clips`]. Multiply premultiplied fragment
    /// output by `rounded_clip_coverage(position.xy)`, using fragment
    /// `@builtin(position)`. Call before discard in uniform control flow.
    /// At zero capacity this returns constant coverage with no resource declarations;
    /// omit group one from the pipeline layout in that case.
    pub fn rounded_clip_shader(&self) -> String {
        let capacity = self.rounded_clip_capacity();
        if capacity == 0 {
            return "fn rounded_clip_coverage(position: vec2<f32>) -> f32 { return 1.0; }".into();
        }
        include_str!("clip_stack.wgsl").replace(
            "array<RoundedClipRecord, 64>",
            &format!("array<RoundedClipRecord, {capacity}>"),
        )
    }

    /// Uniform layout for [`Self::rounded_clip_shader`] at shader group one.
    /// Shared by clones of this context; may be retained and reused across custom materials.
    /// At zero capacity this is an empty layout. Omit it from pipeline layouts,
    /// particularly on devices that support only one bind group.
    pub fn rounded_clip_layout(&self) -> wgpu::BindGroupLayout {
        self.clip_layout
            .get_or_init(|| {
                let capacity = self.rounded_clip_capacity();
                let entries = (capacity > 0).then(|| wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(stack_size(capacity)),
                    },
                    count: None,
                });
                self.device()
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("RXUI drawing rounded clip stack"),
                        entries: entries.as_slice(),
                    })
            })
            .clone()
    }
    pub(crate) fn clip_group(&self, buffer: Option<(&wgpu::Buffer, u64)>) -> wgpu::BindGroup {
        let entry = buffer.map(|(buffer, offset)| wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset,
                size: std::num::NonZeroU64::new(stack_size(self.rounded_clip_capacity())),
            }),
        });
        self.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("RXUI drawing rounded clips"),
            layout: &self.rounded_clip_layout(),
            entries: entry.as_slice(),
        })
    }
}

// Device limits are immutable. Evaluate once for each created/imported context,
// rather than querying wgpu on every primitive record.
pub(crate) fn capacity(limits: &wgpu::Limits) -> usize {
    // Reserve one fragment uniform for brush resources so every built-in
    // renderer can use the same clipping capability and shader interface.
    if limits.max_bind_groups < 2 || limits.max_uniform_buffers_per_shader_stage < 2 {
        return 0;
    }
    let bytes = limits
        .max_buffer_size
        .min(limits.max_uniform_buffer_binding_size);
    ((bytes.saturating_sub(16) / SIZE) as usize).min(MAX_ROUNDED_CLIPS)
}
