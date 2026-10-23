use crate::{Vertex, VertexLayout};
use bytemuck::Pod;
use wgpu::util::DeviceExt;

use crate::{Error, GraphicsContext};

/// CPU bytes and layout for one uploaded vertex or instance stream.
#[derive(Debug)]
pub struct VertexStream<'a> {
    /// Tightly represented bytes; must contain a whole number of strides.
    pub bytes: &'a [u8],
    /// Shader attribute layout.
    pub layout: VertexLayout,
}
impl<'a> VertexStream<'a> {
    /// Borrows typed plain-data vertices or instances without allocating or copying.
    pub fn new<T: Pod>(vertices: &'a [T], layout: VertexLayout) -> Self {
        Self {
            bytes: bytemuck::cast_slice(vertices),
            layout,
        }
    }
}

/// Optional index data. Both widths are supported without converting to Uint32.
#[derive(Clone, Copy, Debug)]
pub enum MeshIndices<'a> {
    /// Sixteen-bit indices.
    U16(&'a [u16]),
    /// Thirty-two-bit indices.
    U32(&'a [u32]),
}
/// Upload configuration for custom layouts, multiple streams, and dynamic geometry.
#[derive(Debug)]
pub struct MeshOptions<'a> {
    /// Vertex and instance streams in shader slot order.
    pub streams: &'a [VertexStream<'a>],
    /// Optional indices. Absence enables nonindexed draws.
    pub indices: Option<MeshIndices<'a>>,
    /// Primitive topology, which must match the material's pipeline topology.
    pub topology: wgpu::PrimitiveTopology,
    /// Adds COPY_DST to owned buffers, enabling explicit updates.
    pub dynamic: bool,
}
impl<'a> MeshOptions<'a> {
    /// Selects nonindexed triangle geometry with immutable buffers.
    pub const fn new(streams: &'a [VertexStream<'a>]) -> Self {
        Self {
            streams,
            indices: None,
            topology: wgpu::PrimitiveTopology::TriangleList,
            dynamic: false,
        }
    }
    /// Selects index storage without widening Uint16 indices.
    pub const fn indices(mut self, indices: MeshIndices<'a>) -> Self {
        self.indices = Some(indices);
        self
    }
    /// Selects primitive topology.
    pub const fn topology(mut self, topology: wgpu::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }
    /// Enables or disables explicit buffer updates.
    pub const fn dynamic(mut self, enabled: bool) -> Self {
        self.dynamic = enabled;
        self
    }
}
/// Application-owned vertex buffer range and layout for importing geometry.
#[derive(Debug)]
pub struct MeshVertexBuffer<'a> {
    /// Existing vertex buffer, on the context's device.
    pub buffer: &'a wgpu::Buffer,
    /// Byte range retained by the mesh.
    pub range: std::ops::Range<u64>,
    /// Vertex or instance attribute layout.
    pub layout: VertexLayout,
}
/// Application-owned index buffer range and index width.
#[derive(Debug)]
pub struct MeshIndexBuffer<'a> {
    /// Existing index buffer, on the context's device.
    pub buffer: &'a wgpu::Buffer,
    /// Byte range containing indices.
    pub range: std::ops::Range<u64>,
    /// Index representation.
    pub format: wgpu::IndexFormat,
}
/// Draw range for either indexed or nonindexed geometry.
/// Indexed ranges select indices; nonindexed ranges select vertices.
#[derive(Clone, Debug)]
pub struct MeshDraw {
    /// Index or vertex range.
    pub range: std::ops::Range<u32>,
    /// Instance range, checked against supplied instance streams.
    pub instances: std::ops::Range<u32>,
    /// Added to each index; ignored for nonindexed drawing.
    /// Imported index contents and base-vertex bounds remain application-controlled.
    pub base_vertex: i32,
}
impl MeshDraw {
    /// Selects a geometry range with one instance and zero base vertex.
    pub fn new(range: std::ops::Range<u32>) -> Self {
        Self {
            range,
            instances: 0..1,
            base_vertex: 0,
        }
    }
    /// Selects instance indices.
    pub fn instances(mut self, instances: std::ops::Range<u32>) -> Self {
        self.instances = instances;
        self
    }
    /// Selects an indexed base vertex.
    pub fn base_vertex(mut self, base: i32) -> Self {
        self.base_vertex = base;
        self
    }
}
#[derive(Debug)]
struct Stream {
    buffer: wgpu::Buffer,
    range: std::ops::Range<u64>,
    layout: VertexLayout,
}
/// Device-bound geometry retaining vertex/instance streams and optional index storage.
///
/// [`GraphicsContext::create_mesh`] uploads standard indexed triangles. Custom
/// layouts, Uint16/Uint32 indices, nonindexed geometry, and updates use
/// [`GraphicsContext::create_mesh_with_options`]. Existing buffer ranges can be
/// retained through [`GraphicsContext::create_mesh_from_buffers`]. Drawing does
/// not reupload geometry; explicit dynamic buffers permit queued updates.
/// wgpu retains resources used by in-flight commands after this mesh is dropped.
#[derive(Debug)]
pub struct Mesh {
    pub(crate) device: wgpu::Device,
    pub(crate) instance: wgpu::Instance,

    vertex_buffer: wgpu::Buffer,
    index_buffer: Option<wgpu::Buffer>,
    indexed: bool,
    index_format: wgpu::IndexFormat,
    index_range: std::ops::Range<u64>,
    streams: Vec<Stream>,
    topology: wgpu::PrimitiveTopology,
    instance_count: Option<u32>,
    vertex_count: u32,
    index_count: u32,
}

impl Mesh {
    /// Validates and uploads a fixed-format vertex buffer and Uint32 index buffer.
    ///
    /// Every three indices describe one triangle. Vertex colors use straight
    /// alpha; the fragment shader premultiplies them before blending.
    pub(crate) fn upload(
        graphics: &GraphicsContext,
        vertices: &[Vertex],
        indices: &[u32],
    ) -> Result<Self, Error> {
        validate(
            vertices,
            indices,
            graphics.device().limits().max_buffer_size,
        )?;
        let vertex_buffer =
            graphics
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("RXUI drawing mesh vertices"),
                    contents: bytemuck::cast_slice(vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            graphics
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("RXUI drawing mesh indices"),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
        let streams = vec![Stream {
            buffer: vertex_buffer.clone(),
            range: 0..vertex_buffer.size(),
            layout: VertexLayout::new(&Vertex::layout()),
        }];
        let index_range = 0..index_buffer.size();
        Ok(Self {
            device: graphics.device().clone(),
            instance: graphics.instance().clone(),

            vertex_buffer,
            index_buffer: Some(index_buffer),
            indexed: true,
            index_format: wgpu::IndexFormat::Uint32,
            index_range,
            streams,
            topology: wgpu::PrimitiveTopology::TriangleList,
            instance_count: None,
            vertex_count: vertices.len() as u32,
            index_count: indices.len() as u32,
        })
    }

    pub(crate) fn upload_options(g: &GraphicsContext, o: MeshOptions<'_>) -> Result<Self, Error> {
        if o.streams.is_empty() {
            return Err(Error::EmptyVertices);
        }
        if o.streams.len() > g.device().limits().max_vertex_buffers as usize {
            return Err(Error::InvalidGeometry);
        }
        // Validate all stream metadata before creating any GPU buffers.
        for stream in o.streams {
            validate_stream(g, stream.bytes.len() as u64, &stream.layout)?;
        }
        let vertices = o
            .streams
            .iter()
            .filter(|s| s.layout.step_mode == wgpu::VertexStepMode::Vertex)
            .map(|s| s.bytes.len() as u64 / s.layout.stride)
            .min()
            .ok_or(Error::InvalidGeometry)?;
        if let Some(indices) = o.indices {
            let invalid = match indices {
                MeshIndices::U16(i) => i.iter().any(|&v| {
                    u64::from(v) >= vertices && !(o.topology.is_strip() && v == u16::MAX)
                }),
                MeshIndices::U32(i) => i.iter().any(|&v| {
                    u64::from(v) >= vertices && !(o.topology.is_strip() && v == u32::MAX)
                }),
            };
            if invalid {
                return Err(Error::InvalidGeometry);
            }
        }
        let data = o.indices.map(|i| match i {
            MeshIndices::U16(i) => (bytemuck::cast_slice(i), wgpu::IndexFormat::Uint16),
            MeshIndices::U32(i) => (bytemuck::cast_slice(i), wgpu::IndexFormat::Uint32),
        });
        if let Some((bytes, _)) = data
            && (bytes.is_empty() || bytes.len() as u64 > g.device().limits().max_buffer_size)
        {
            return Err(Error::InvalidGeometry);
        }
        let usage = wgpu::BufferUsages::VERTEX
            | if o.dynamic {
                wgpu::BufferUsages::COPY_DST
            } else {
                wgpu::BufferUsages::empty()
            };
        let owned: Vec<_> = o
            .streams
            .iter()
            .map(|s| {
                g.device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("RXUI drawing vertex stream"),
                        contents: s.bytes,
                        usage,
                    })
            })
            .collect();
        let streams: Vec<_> = owned
            .iter()
            .zip(o.streams)
            .map(|(b, s)| MeshVertexBuffer {
                buffer: b,
                range: 0..s.bytes.len() as u64,
                layout: s.layout.clone(),
            })
            .collect();
        let index = data.map(|(bytes, format)| {
            (
                g.device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("RXUI drawing indices"),
                        contents: bytes,
                        usage: wgpu::BufferUsages::INDEX
                            | if o.dynamic {
                                wgpu::BufferUsages::COPY_DST
                            } else {
                                wgpu::BufferUsages::empty()
                            },
                    }),
                bytes.len() as u64,
                format,
            )
        });
        let descriptor = index
            .as_ref()
            .map(|(buffer, size, format)| MeshIndexBuffer {
                buffer,
                range: 0..*size,
                format: *format,
            });
        Self::from_buffers(g, &streams, descriptor, o.topology)
    }

    pub(crate) fn from_buffers(
        g: &GraphicsContext,
        streams: &[MeshVertexBuffer<'_>],
        index: Option<MeshIndexBuffer<'_>>,
        topology: wgpu::PrimitiveTopology,
    ) -> Result<Self, Error> {
        if streams.is_empty() || streams.len() > g.device().limits().max_vertex_buffers as usize {
            return Err(Error::InvalidGeometry);
        }
        for stream in streams {
            let length = stream
                .range
                .end
                .checked_sub(stream.range.start)
                .ok_or(Error::InvalidGeometry)?;
            validate_stream(g, length, &stream.layout)?;
            if stream.range.end > stream.buffer.size()
                || stream.range.start % 4 != 0
                || !stream.buffer.usage().contains(wgpu::BufferUsages::VERTEX)
            {
                return Err(Error::InvalidGeometry);
            }
        }
        let vertex_count = streams
            .iter()
            .filter(|s| s.layout.step_mode == wgpu::VertexStepMode::Vertex)
            .map(|s| (s.range.end - s.range.start) / s.layout.stride)
            .min()
            .ok_or(Error::InvalidGeometry)?;
        let instance_count = streams
            .iter()
            .filter(|s| s.layout.step_mode == wgpu::VertexStepMode::Instance)
            .map(|s| ((s.range.end - s.range.start) / s.layout.stride) as u32)
            .min();
        let (index_buffer, index_range, index_format, index_count, indexed) = if let Some(i) = index
        {
            let stride = match i.format {
                wgpu::IndexFormat::Uint16 => 2,
                wgpu::IndexFormat::Uint32 => 4,
            };
            let length = i
                .range
                .end
                .checked_sub(i.range.start)
                .ok_or(Error::InvalidGeometry)?;
            if length == 0
                || length % stride != 0
                || i.range.start % stride != 0
                || i.range.end > i.buffer.size()
                || !i.buffer.usage().contains(wgpu::BufferUsages::INDEX)
                || length / stride > u32::MAX as u64
            {
                return Err(Error::InvalidGeometry);
            }
            (
                Some(i.buffer.clone()),
                i.range,
                i.format,
                (length / stride) as u32,
                true,
            )
        } else {
            (None, 0..0, wgpu::IndexFormat::Uint32, 0, false)
        };
        let vertex_buffer = streams[0].buffer.clone();
        Ok(Self {
            device: g.device().clone(),
            instance: g.instance().clone(),

            vertex_buffer,
            index_buffer,
            vertex_count: vertex_count as u32,
            index_count,
            indexed,
            index_format,
            index_range,
            streams: streams
                .iter()
                .map(|s| Stream {
                    buffer: s.buffer.clone(),
                    range: s.range.clone(),
                    layout: s.layout.clone(),
                })
                .collect(),
            topology,
            instance_count,
        })
    }

    /// Returns a draw selecting all indices or vertices, with one instance.
    pub fn full_draw(&self) -> MeshDraw {
        MeshDraw::new(
            0..if self.indexed {
                self.index_count
            } else {
                self.vertex_count
            },
        )
    }

    /// Returns vertex layouts in stream order.
    pub fn vertex_layouts(&self) -> impl Iterator<Item = &VertexLayout> {
        self.streams.iter().map(|s| &s.layout)
    }

    /// Updates bytes within an explicitly dynamic vertex/instance stream.
    /// Queue writes execute before the next submission, not between recorded draws.
    /// Layout and stream capacity remain fixed; alignment and bounds are checked.
    pub fn write_vertices(
        &self,
        g: &GraphicsContext,
        slot: usize,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), Error> {
        if self.device != *g.device() || self.instance != *g.instance() {
            return Err(Error::DeviceMismatch);
        }
        let s = self.streams.get(slot).ok_or(Error::InvalidGeometry)?;
        let start = s
            .range
            .start
            .checked_add(offset)
            .ok_or(Error::InvalidGeometry)?;
        if start % 4 != 0
            || !bytes.len().is_multiple_of(4)
            || start
                .checked_add(bytes.len() as u64)
                .is_none_or(|e| e > s.range.end)
            || !s.buffer.usage().contains(wgpu::BufferUsages::COPY_DST)
        {
            return Err(Error::InvalidGeometry);
        }
        if !bytes.is_empty() {
            g.queue().write_buffer(&s.buffer, start, bytes);
        }
        Ok(())
    }

    /// Updates an explicitly dynamic index buffer without changing its width or capacity.
    pub fn write_indices(
        &self,
        g: &GraphicsContext,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), Error> {
        if self.device != *g.device() || self.instance != *g.instance() {
            return Err(Error::DeviceMismatch);
        }
        let start = self
            .index_range
            .start
            .checked_add(offset)
            .ok_or(Error::InvalidGeometry)?;
        if !self.indexed
            || start % 4 != 0
            || !bytes.len().is_multiple_of(4)
            || start
                .checked_add(bytes.len() as u64)
                .is_none_or(|e| e > self.index_range.end)
            || self
                .index_buffer
                .as_ref()
                .is_none_or(|b| !b.usage().contains(wgpu::BufferUsages::COPY_DST))
        {
            return Err(Error::InvalidGeometry);
        }
        if !bytes.is_empty() {
            g.queue()
                .write_buffer(self.index_buffer.as_ref().unwrap(), start, bytes);
        }
        Ok(())
    }

    pub(crate) fn validate_draw(&self, m: &crate::Material, d: &MeshDraw) -> Result<(), Error> {
        if (self.indexed
            && self.topology.is_strip()
            && m.strip_index_format != Some(self.index_format))
            || self.topology != m.topology
            || !self.vertex_layouts().eq(m.vertex_layouts.iter())
        {
            return Err(Error::InvalidGeometry);
        }
        self.validate_range(d)
    }

    pub(crate) fn validate_range(&self, d: &MeshDraw) -> Result<(), Error> {
        if d.range.start > d.range.end
            || d.range.end
                > if self.indexed {
                    self.index_count
                } else {
                    self.vertex_count
                }
            || d.instances.start > d.instances.end
            || self.instance_count.is_some_and(|n| d.instances.end > n)
        {
            return Err(Error::InvalidGeometry);
        }
        Ok(())
    }
    pub(crate) fn record(&self, pass: &mut crate::RenderPass<'_>, d: &MeshDraw) {
        self.bind(pass);
        self.record_draw(pass, d);
    }
    pub(crate) fn bind(&self, pass: &mut crate::RenderPass<'_>) {
        for (slot, s) in self.streams.iter().enumerate() {
            pass.set_vertex_buffer(slot as u32, &s.buffer, s.range.clone());
        }
        if self.indexed {
            pass.set_index_buffer(
                self.index_buffer.as_ref().unwrap(),
                self.index_range.clone(),
                self.index_format,
            );
        }
    }
    #[inline]
    pub(crate) fn record_draw(&self, pass: &mut crate::RenderPass<'_>, d: &MeshDraw) {
        if self.indexed {
            pass.draw_indexed(d.range.clone(), d.base_vertex, d.instances.clone());
        } else {
            pass.draw(d.range.clone(), d.instances.clone());
        }
    }

    /// Returns the number of uploaded vertices.
    pub const fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    /// Returns the number of uploaded Uint32 indices.
    pub const fn index_count(&self) -> u32 {
        self.index_count
    }

    /// Returns the vertex buffer for custom wgpu rendering.
    pub fn vertex_buffer(&self) -> &wgpu::Buffer {
        &self.vertex_buffer
    }

    /// Returns a vertex or instance stream's retained buffer by slot.
    pub fn vertex_buffer_at(&self, slot: usize) -> Option<&wgpu::Buffer> {
        self.streams.get(slot).map(|s| &s.buffer)
    }

    /// Borrows the exact retained vertex/instance range for custom rendering.
    pub fn vertex_buffer_slice(&self, slot: usize) -> Option<wgpu::BufferSlice<'_>> {
        self.streams
            .get(slot)
            .map(|s| s.buffer.slice(s.range.clone()))
    }

    /// Borrows the exact retained index range, if geometry is indexed.
    pub fn index_buffer_slice(&self) -> Option<wgpu::BufferSlice<'_>> {
        self.index_buffer
            .as_ref()
            .map(|b| b.slice(self.index_range.clone()))
    }

    /// Returns the index width, or `None` for nonindexed geometry.
    pub fn index_format(&self) -> Option<wgpu::IndexFormat> {
        self.indexed.then_some(self.index_format)
    }

    /// Returns the retained index buffer, or `None` for nonindexed geometry.
    pub fn index_buffer(&self) -> Option<&wgpu::Buffer> {
        self.index_buffer.as_ref()
    }
}

fn validate(vertices: &[Vertex], indices: &[u32], max_buffer_size: u64) -> Result<(), Error> {
    if vertices.is_empty() {
        return Err(Error::EmptyVertices);
    }
    if indices.is_empty() || !indices.len().is_multiple_of(3) {
        return Err(Error::InvalidIndexCount {
            count: indices.len(),
        });
    }
    if vertices.len() > u32::MAX as usize
        || indices.len() > u32::MAX as usize
        || std::mem::size_of_val(vertices) as u64 > max_buffer_size
        || std::mem::size_of_val(indices) as u64 > max_buffer_size
    {
        return Err(Error::MeshTooLarge);
    }
    for (index, vertex) in vertices.iter().enumerate() {
        if !vertex
            .position
            .iter()
            .chain(&vertex.color)
            .all(|value| value.is_finite())
        {
            return Err(Error::InvalidVertex { index });
        }
    }
    for &index in indices {
        if index as usize >= vertices.len() {
            return Err(Error::IndexOutOfBounds {
                index,
                vertex_count: vertices.len(),
            });
        }
    }
    Ok(())
}

fn validate_stream(g: &GraphicsContext, length: u64, layout: &VertexLayout) -> Result<(), Error> {
    let limits = g.device().limits();
    if layout.stride == 0
        || !layout.stride.is_multiple_of(4)
        || layout.stride > u64::from(limits.max_vertex_buffer_array_stride)
        || length == 0
        || !length.is_multiple_of(layout.stride)
        || length > limits.max_buffer_size
        || length / layout.stride > u32::MAX as u64
        || layout.attributes.iter().any(|a| {
            a.offset
                .checked_add(a.format.size())
                .is_none_or(|e| e > layout.stride)
        })
    {
        return Err(Error::InvalidGeometry);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_meshes_before_gpu_upload() {
        let vertex = Vertex::new([0.0; 3], [1.0; 4]);
        assert!(matches!(
            validate(&[], &[0, 0, 0], 1024),
            Err(Error::EmptyVertices)
        ));
        assert!(matches!(
            validate(&[vertex], &[0, 0], 1024),
            Err(Error::InvalidIndexCount { .. })
        ));
        assert!(matches!(
            validate(&[vertex], &[0, 1, 0], 1024),
            Err(Error::IndexOutOfBounds { index: 1, .. })
        ));
        assert!(matches!(
            validate(&[vertex], &[0, 0, 0], 8),
            Err(Error::MeshTooLarge)
        ));
        let invalid = Vertex::new([f32::NAN, 0.0, 0.0], [1.0; 4]);
        assert!(matches!(
            validate(&[invalid], &[0, 0, 0], 1024),
            Err(Error::InvalidVertex { index: 0 })
        ));
    }
}
