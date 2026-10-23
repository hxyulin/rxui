use bytemuck::{Pod, Zeroable};

/// Standard position/color vertex format for convenient mesh drawing.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// Position, interpreted as clip space by default: X/Y -1 to 1 and Z 0 to 1.
    /// Custom material vertex shaders can transform this into clip space.
    pub position: [f32; 3],
    /// Linear RGB and straight alpha, interpolated across each triangle.
    pub color: [f32; 4],
}

impl Vertex {
    /// Creates a colored clip-space vertex.
    pub const fn new(position: [f32; 3], color: [f32; 4]) -> Self {
        Self { position, color }
    }

    /// Returns the standard position/color layout for custom wgpu pipelines.
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

/// Owned vertex layout, reusable by geometry and materials.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VertexLayout {
    /// Bytes between vertices or instances.
    pub stride: u64,
    /// Whether this stream advances per vertex or per instance.
    pub step_mode: wgpu::VertexStepMode,
    /// Shader locations, formats, and offsets.
    pub attributes: Vec<wgpu::VertexAttribute>,
}
impl VertexLayout {
    /// Copies a wgpu layout into an owned description.
    pub fn new(layout: &wgpu::VertexBufferLayout<'_>) -> Self {
        Self {
            stride: layout.array_stride,
            step_mode: layout.step_mode,
            attributes: layout.attributes.to_vec(),
        }
    }
    /// Borrows this layout as a wgpu vertex buffer descriptor.
    pub fn as_wgpu(&self) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: self.stride,
            step_mode: self.step_mode,
            attributes: &self.attributes,
        }
    }
}
