use crate::{Error, GraphicsContext};
use bytemuck::{Pod, Zeroable};

const GAP: u32 = 0x7fc0_0001;

/// An eight-byte XY sample in local data coordinates, or an explicit connection break.
///
/// Positions must be finite. Use [`Self::gap`] for missing data; ordinary NaNs and
/// infinities are rejected during updates. Gap bits are tested before conversion
/// to floats in the shader. Large timestamps should be rebased in f64 by the
/// application before conversion to these f32 coordinates.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Point2D([f32; 2]);
impl Point2D {
    /// Creates an XY sample; buffer updates validate its coordinates.
    pub const fn new(position: [f32; 2]) -> Self {
        Self(position)
    }
    /// An explicit gap. Polylines break connections and markers skip this sample.
    pub const fn gap() -> Self {
        Self([f32::from_bits(GAP); 2])
    }
    /// Returns the coordinates, or None for an explicit gap.
    #[inline]
    pub fn position(self) -> Option<[f32; 2]> {
        (!self.is_gap()).then_some(self.0)
    }
    fn is_gap(self) -> bool {
        self.0.map(f32::to_bits) == [GAP; 2]
    }
}
impl From<[f32; 2]> for Point2D {
    fn from(value: [f32; 2]) -> Self {
        Self::new(value)
    }
}

/// Fixed-capacity point storage configuration.
#[derive(Clone, Copy, Debug)]
pub struct PointBufferOptions {
    /// Maximum retained points, including gaps. Must be nonzero and fit device limits.
    pub capacity: usize,
    /// Whether append evicts oldest points when capacity is reached.
    pub ring: bool,
}
impl PointBufferOptions {
    /// Linear storage: append beyond capacity fails without changing the resource.
    pub const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            ring: false,
        }
    }
    /// Bounded rolling storage. Append preserves newest samples in logical order.
    pub const fn ring(mut self) -> Self {
        self.ring = true;
        self
    }
}

/// Counts from one successful append, including ring eviction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointAppend {
    /// Number of input points actually uploaded (at most capacity in ring mode).
    pub written: usize,
    /// Old/input points removed from the resulting logical sequence.
    pub evicted: usize,
}

/// Retained, mutable GPU samples shared by polyline and marker renderers.
///
/// Created through [`GraphicsContext::create_point_buffer`]. Capacity is fixed;
/// updates never reallocate or move existing samples. Linear append fails on
/// overflow. Ring append evicts oldest points and wraps physical writes, while
/// draw ranges and [`Self::write`] always use logical oldest-to-newest indices.
/// There is no CPU point mirror, tessellation, or hidden data reduction.
///
/// **Updates are queue writes, not recorded snapshots.** They execute before the
/// next submission on this queue. Update before recording/submitting a frame;
/// writing between recorded draws does not give those draws different versions.
/// Already-submitted frames preserve queue ordering. Use separate buffers when
/// simultaneously recording independently updated versions. Recorded wgpu commands
/// keep GPU handles alive if this resource is dropped before submission.
///
/// Finite validation scales with supplied samples. Drawing uses cached conservative
/// coordinate bounds and never scans data. Partial writes/ring appends only expand
/// these bounds; `replace` and `clear` reset them. Drawing extreme transforms can
/// therefore reject a conservative bound even if the selected subset would fit.
#[derive(Debug)]
pub struct PointBuffer {
    pub(crate) graphics: GraphicsContext,
    buffer: wgpu::Buffer,
    pub(crate) group: wgpu::BindGroup,
    pub(crate) extent: [f64; 2],
    pub(crate) head: usize,
    len: usize,
    capacity: usize,
    ring: bool,
    uploaded: u64,
    writes: u64,
}
impl PointBuffer {
    /// Number of logical samples, including gaps.
    pub fn len(&self) -> usize {
        self.len
    }
    /// Whether there are no logical samples.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Fixed retained capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    /// Whether append evicts oldest samples at capacity.
    pub fn is_ring(&self) -> bool {
        self.ring
    }
    /// Total sample bytes queued by successful updates; creation queues none.
    pub fn uploaded_bytes(&self) -> u64 {
        self.uploaded
    }
    /// Number of queue buffer writes. Wrapped updates use at most two writes.
    pub fn write_count(&self) -> u64 {
        self.writes
    }
    /// Allocated sample storage bytes (eight per capacity slot).
    pub fn buffer_bytes(&self) -> u64 {
        self.buffer.size()
    }
    /// Borrows raw STORAGE/COPY_DST/COPY_SRC storage for interoperability/readback.
    /// Layout is eight-byte XY f32 pairs with the explicit gap bit pattern.
    /// Raw writes bypass finite/bounds checks and logical-length tracking; the
    /// application must preserve those contracts. Draw ranges still use logical indices.
    pub fn as_wgpu(&self) -> &wgpu::Buffer {
        &self.buffer
    }
    /// Physical index of the oldest sample, for raw interoperability.
    pub fn physical_start(&self) -> usize {
        self.head
    }
    /// Empties the logical sequence without a GPU clear, allocation, or upload.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
        self.extent = [0.; 2];
    }
    /// Replaces the logical sequence, starting at physical zero. Capacity stays fixed.
    /// Invalid input/overflow leaves metadata and queued data unchanged.
    pub fn replace(&mut self, points: &[Point2D]) -> Result<(), Error> {
        if points.len() > self.capacity {
            return Err(Error::PointCapacityExceeded);
        }
        let extent = validate(points)?;
        self.upload(0, points);
        self.head = 0;
        self.len = points.len();
        self.extent = extent;
        Ok(())
    }
    /// Overwrites existing logical samples. Does not extend length or allocate.
    /// The entire input and range are checked before either wrapped write is queued.
    pub fn write(&mut self, start: usize, points: &[Point2D]) -> Result<(), Error> {
        if start
            .checked_add(points.len())
            .is_none_or(|end| end > self.len)
        {
            return Err(Error::InvalidPointRange);
        }
        let extent = validate(points)?;
        self.upload((self.head + start) % self.capacity, points);
        for (old, new) in self.extent.iter_mut().zip(extent) {
            *old = old.max(new);
        }
        Ok(())
    }
    /// Appends new samples. Ring storage retains the newest capacity samples.
    /// Oversized ring input is fully validated, then only its retained suffix uploads.
    /// Returns retained upload/eviction counts. Empty append queues no writes.
    pub fn append(&mut self, points: &[Point2D]) -> Result<PointAppend, Error> {
        let total = self
            .len
            .checked_add(points.len())
            .ok_or(Error::PointCapacityExceeded)?;
        if !self.ring && total > self.capacity {
            return Err(Error::PointCapacityExceeded);
        }
        let extent = validate(points)?;
        if points.len() >= self.capacity {
            let kept = &points[points.len() - self.capacity..];
            let extent = validate(kept)?;
            self.upload(0, kept);
            self.head = 0;
            self.len = self.capacity;
            self.extent = extent;
        } else {
            self.upload((self.head + self.len) % self.capacity, points);
            self.head = (self.head + total.saturating_sub(self.capacity)) % self.capacity;
            self.len = total.min(self.capacity);
            for (old, new) in self.extent.iter_mut().zip(extent) {
                *old = old.max(new);
            }
        }
        Ok(PointAppend {
            written: points.len().min(self.capacity),
            evicted: total.saturating_sub(self.capacity),
        })
    }
    fn upload(&mut self, physical: usize, points: &[Point2D]) {
        if points.is_empty() {
            return;
        }
        let split = points.len().min(self.capacity - physical);
        self.graphics.queue().write_buffer(
            &self.buffer,
            (physical * 8) as u64,
            bytemuck::cast_slice(&points[..split]),
        );
        self.writes = self.writes.saturating_add(1);
        if split < points.len() {
            self.graphics.queue().write_buffer(
                &self.buffer,
                0,
                bytemuck::cast_slice(&points[split..]),
            );
            self.writes = self.writes.saturating_add(1);
        }
        self.uploaded = self.uploaded.saturating_add((points.len() * 8) as u64);
    }
    pub(crate) fn create(
        graphics: &GraphicsContext,
        options: PointBufferOptions,
    ) -> Result<Self, Error> {
        check_support(graphics)?;
        let limits = graphics.device().limits();
        let bytes = options
            .capacity
            .checked_mul(8)
            .ok_or(Error::PointBufferTooLarge)?;
        if options.capacity == 0 {
            return Err(Error::InvalidPointCapacity);
        }
        if options.capacity > u32::MAX as usize / 64
            || bytes as u64
                > limits
                    .max_buffer_size
                    .min(limits.max_storage_buffer_binding_size)
        {
            return Err(Error::PointBufferTooLarge);
        }
        let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("RXUI drawing point samples"),
            size: bytes as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let group = graphics
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("RXUI drawing point samples"),
                layout: &layout(graphics.device()),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
        Ok(Self {
            graphics: graphics.clone(),
            buffer,
            group,
            extent: [0.; 2],
            head: 0,
            len: 0,
            capacity: options.capacity,
            ring: options.ring,
            uploaded: 0,
            writes: 0,
        })
    }
}
fn validate(points: &[Point2D]) -> Result<[f64; 2], Error> {
    let mut extent = [0f64; 2];
    for (index, p) in points.iter().enumerate() {
        if p.is_gap() {
            continue;
        }
        if !p.0.into_iter().all(f32::is_finite) {
            return Err(Error::InvalidPoint { index });
        }
        for (e, v) in extent.iter_mut().zip(p.0) {
            *e = e.max(f64::from(v).abs());
        }
    }
    Ok(extent)
}
pub(crate) fn check_support(graphics: &GraphicsContext) -> Result<(), Error> {
    let l = graphics.device().limits();
    if !graphics
        .adapter()
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::VERTEX_STORAGE)
        || l.max_storage_buffers_per_shader_stage < 1
        || l.max_bind_groups < 1
        || l.max_storage_buffer_binding_size < 8
        || l.max_buffer_size < 80
        || l.max_vertex_attributes < 5
        || l.max_vertex_buffers < 1
        || l.max_vertex_buffer_array_stride < 80
        || l.max_inter_stage_shader_variables < 4
    {
        return Err(Error::UnsupportedPointLimits);
    }
    Ok(())
}
pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("RXUI drawing point layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(8),
            },
            count: None,
        }],
    })
}
