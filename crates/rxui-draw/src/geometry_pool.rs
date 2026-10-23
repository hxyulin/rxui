//! Reuse completed geometry allocations without mutating any retained resource.

use crate::GraphicsContext;
use std::sync::{Arc, Mutex, Weak};

// This bounds recycled storage, including buffers waiting for GPU completion.
// Live prepared texts keep their own allocations.
const MAX_IDLE_BYTES: u64 = 1024 * 1024;
const MAX_IDLE_BUFFERS: usize = 64;
const MAX_POOLED_BUFFER: u64 = 64 * 1024;

#[derive(Debug, Default)]
pub(crate) struct GeometryPool {
    buffers: Vec<(wgpu::Buffer, Weak<()>)>,
    bytes: u64,
}

#[derive(Debug)]
pub(crate) struct Geometry {
    pub buffer: wgpu::Buffer,
    pub lease: Arc<()>,
    pool: Weak<Mutex<GeometryPool>>,
}

impl GeometryPool {
    /// Uploads `parts` back to back into an idle completed buffer of this pool,
    /// or a new one. Every buffer in a pool must use the same `usage`.
    pub fn upload(
        pool: &Arc<Mutex<Self>>,
        graphics: &GraphicsContext,
        label: &'static str,
        usage: wgpu::BufferUsages,
        parts: &[&[u8]],
    ) -> (Geometry, bool) {
        let size = parts.iter().map(|part| part.len() as u64).sum::<u64>();
        let reusable = {
            let mut idle = pool.lock().unwrap_or_else(|e| e.into_inner());
            let best = idle
                .buffers
                .iter()
                .enumerate()
                .filter(|(_, (b, lease))| b.size() >= size && lease.strong_count() == 0)
                .min_by_key(|(_, (b, _))| b.size())
                .map(|(i, _)| i);
            best.map(|i| {
                let (buffer, _) = idle.buffers.swap_remove(i);
                idle.bytes -= buffer.size();
                buffer
            })
        };
        let reused = reusable.is_some();
        let buffer = if let Some(buffer) = reusable {
            let mut offset = 0;
            for part in parts {
                graphics.queue().write_buffer(&buffer, offset, part);
                offset += part.len() as u64;
            }
            buffer
        } else {
            // Keep large geometry exact-sized. Small geometry can grow within a bucket.
            let capacity = if size <= MAX_POOLED_BUFFER {
                size.next_power_of_two()
            } else {
                size
            }
            .min(graphics.device().limits().max_buffer_size & !3);
            let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: true,
            });
            {
                let mut mapped = buffer.slice(..).get_mapped_range_mut().unwrap();
                let mut offset = 0;
                for part in parts {
                    mapped
                        .slice(offset..offset + part.len())
                        .copy_from_slice(part);
                    offset += part.len();
                }
            }
            buffer.unmap();
            buffer
        };
        (
            Geometry {
                buffer,
                lease: Arc::new(()),
                pool: Arc::downgrade(pool),
            },
            reused,
        )
    }
}

impl Drop for Geometry {
    fn drop(&mut self) {
        let size = self.buffer.size();
        if size > MAX_POOLED_BUFFER {
            return;
        }
        if let Some(pool) = self.pool.upgrade() {
            let mut idle = pool.lock().unwrap_or_else(|e| e.into_inner());
            if idle.buffers.len() < MAX_IDLE_BUFFERS && idle.bytes + size <= MAX_IDLE_BYTES {
                idle.bytes += size;
                idle.buffers
                    .push((self.buffer.clone(), Arc::downgrade(&self.lease)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload(
        pool: &Arc<Mutex<GeometryPool>>,
        graphics: &GraphicsContext,
        bytes: &[u8],
    ) -> (Geometry, bool) {
        GeometryPool::upload(pool, graphics, "test", wgpu::BufferUsages::VERTEX, &[bytes])
    }

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn idle_geometry_storage_is_bounded_and_large_buffers_are_released() {
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let pool = Arc::default();
            let bytes = vec![0; 16 * 1024];
            let live: Vec<_> = (0..70)
                .map(|_| upload(&pool, &graphics, &bytes).0)
                .collect();
            assert_eq!(pool.lock().unwrap().bytes, 0);
            drop(live);
            assert_eq!(pool.lock().unwrap().buffers.len(), MAX_IDLE_BUFFERS);
            assert_eq!(pool.lock().unwrap().bytes, MAX_IDLE_BYTES);
            let (large, reused) = upload(&pool, &graphics, &vec![0; 128 * 1024]);
            assert!(!reused);
            drop(large);
            assert_eq!(pool.lock().unwrap().bytes, MAX_IDLE_BYTES);
            let (small, reused) = upload(&pool, &graphics, &[0; 1024]);
            assert!(reused);
            assert_eq!(pool.lock().unwrap().bytes, MAX_IDLE_BYTES - 16 * 1024);
            // Pool ownership is weak: a retained allocation cannot keep idle buffers alive.
            let weak = Arc::downgrade(&pool);
            drop(pool);
            assert!(weak.upgrade().is_none());
            drop(small);
        });
    }
}
