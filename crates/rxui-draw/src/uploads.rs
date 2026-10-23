use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

type IdlePage = (wgpu::Buffer, Vec<u8>, Vec<(u64, wgpu::BindGroup)>);

pub(crate) type UploadPool = Arc<Mutex<IdlePages>>;

/// Recordings an idle page may go unused before it is released. A spike frame's
/// extra pages are freed after this many later recordings; pages that steady
/// frames keep using are never idle that long.
pub(crate) const IDLE_RECORDINGS: u64 = 120;

/// Completed upload pages with their CPU staging, most recently returned last.
#[derive(Debug, Default)]
pub(crate) struct IdlePages {
    pages: Vec<(wgpu::Buffer, Vec<u8>)>,
    // Parallel to pages; binding retention follows the same lease and expiry.
    clip_bindings: Vec<Vec<(u64, wgpu::BindGroup)>>,
    created: u64,
    reused: u64,
    released: u64,
    clip_bindings_created: u64,
    // Recording count when each page was returned, parallel to `pages` and
    // nondecreasing because pages are appended as they return.
    returned: Vec<u64>,
    recordings: u64,
}
impl std::ops::Deref for IdlePages {
    type Target = [(wgpu::Buffer, Vec<u8>)];
    fn deref(&self) -> &Self::Target {
        &self.pages
    }
}
impl IdlePages {
    fn push(&mut self, page: IdlePage) {
        self.pages.push((page.0, page.1));
        self.clip_bindings.push(page.2);
        self.returned.push(self.recordings);
    }
    // Prefers the most recently returned page so surplus pages stay idle and age out.
    fn take(&mut self, length: u64) -> Option<IdlePage> {
        let index = self.pages.iter().rposition(|(b, _)| b.size() >= length)?;
        self.reused += 1;
        self.returned.remove(index);
        let (buffer, bytes) = self.pages.remove(index);
        Some((buffer, bytes, self.clip_bindings.remove(index)))
    }
    fn finish_recording(&mut self) {
        self.recordings += 1;
        let oldest = self.recordings.saturating_sub(IDLE_RECORDINGS);
        if self.returned.first().is_some_and(|&r| r < oldest) {
            let before = self.pages.len();
            let mut index = 0;
            self.pages.retain(|_| {
                index += 1;
                self.returned[index - 1] >= oldest
            });
            let mut index = 0;
            self.clip_bindings.retain(|_| {
                index += 1;
                self.returned[index - 1] >= oldest
            });
            self.returned.retain(|&r| r >= oldest);
            self.released += (before - self.pages.len()) as u64;
        }
    }
}

#[derive(Debug)]
struct Page {
    buffer: wgpu::Buffer,
    bytes: Vec<u8>,
    clip_bindings: Vec<(u64, wgpu::BindGroup)>,
    pool: UploadPool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_leases_deduplicate_small_and_large_collections() {
        let resources: Vec<_> = (0..600).map(Arc::new).collect();
        let weak = Arc::downgrade(&resources[599]);
        let mut uploads = DrawUploads::default();
        for r in &resources {
            uploads.retain(r.clone());
            uploads.retain(r.clone());
        }
        for r in resources.iter().rev() {
            uploads.retain(r.clone());
        }
        assert_eq!(uploads.leases.len(), resources.len());
        drop(resources);
        assert!(weak.upgrade().is_some());
        drop(uploads);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn clip_binding_retention_is_bounded_and_expires_with_its_page() {
        pollster::block_on(async {
            let g = crate::GraphicsContext::headless().await.unwrap();
            let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
            let clip = crate::RoundedClip::new(
                crate::Rect::new(0., 0., 8., 8.),
                crate::CornerRadii::uniform(2.),
            );
            let bytes = crate::clip::stack_bytes(
                &[clip],
                [0., 0., 16., 16., 0., 1.],
                g.rounded_clip_capacity(),
            );
            let alignment = u64::from(g.device().limits().min_uniform_buffer_offset_alignment);
            // Historical placements within one page must not grow a global cache.
            for offset in 0..80 {
                let mut recording = DrawUploads::new(&g.upload_pool);
                if offset > 0 {
                    recording.append(&g, &vec![0; offset * alignment as usize], 4);
                }
                recording.append_clips(&g, &bytes, alignment);
            }
            let filled = g.upload_stats();
            assert_eq!(filled.pages_created, 1);
            assert_eq!(filled.idle_clip_bindings, 64);
            assert_eq!(filled.clip_bindings_created, 80);
            // Lease the cached page while another recording creates a fresh page.
            // Return the fresh page last so only it stays warm.
            let mut cached = DrawUploads::new(&g.upload_pool);
            cached.append(&g, &[0; 4], 4);
            let mut fresh = DrawUploads::new(&g.upload_pool);
            fresh.append(&g, &[0; 4], 4);
            drop(cached);
            drop(fresh);
            for _ in 0..=IDLE_RECORDINGS {
                let mut recording = DrawUploads::new(&g.upload_pool);
                recording.append(&g, &[0; 4], 4);
            }
            let recovered = g.upload_stats();
            assert_eq!(recovered.pages_released, 1);
            assert_eq!(recovered.idle_pages, 1);
            assert_eq!(recovered.idle_clip_bindings, 0);
            assert!(errors.pop().await.is_none());
        });
    }

    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn spike_pages_are_released_after_idle_recordings_and_steady_pages_kept() {
        use crate::{FramebufferOptions, GraphicsContext, Rect, ShapeDraw, ShapeRenderer};
        pollster::block_on(async {
            let g = GraphicsContext::headless().await.unwrap();
            let mut target = g
                .create_framebuffer(FramebufferOptions::new(16, 16))
                .unwrap();
            let mut shapes = ShapeRenderer::new(&g);
            let mut frame = |count: usize| {
                let draws = vec![ShapeDraw::rect(Rect::new(0., 0., 4., 4.), [1.; 4]); count];
                let mut frame = target.begin_frame().unwrap();
                shapes
                    .draw_many(&mut frame.render_pass().begin().unwrap(), &draws)
                    .unwrap();
                frame.finish().unwrap();
            };
            // 5,000 records of 96 bytes need eight 64 KiB pages.
            frame(5000);
            assert_eq!(g.upload_pool.lock().unwrap().len(), 8);
            frame(10);
            let steady = g.upload_pool.lock().unwrap().last().unwrap().0.clone();
            for _ in 0..IDLE_RECORDINGS {
                frame(10);
            }
            let pool = g.upload_pool.lock().unwrap();
            assert_eq!(pool.len(), 1);
            assert_eq!(pool[0].0, steady);
        });
    }
}
impl Drop for Page {
    fn drop(&mut self) {
        self.pool.lock().unwrap_or_else(|e| e.into_inner()).push((
            self.buffer.clone(),
            std::mem::take(&mut self.bytes),
            std::mem::take(&mut self.clip_bindings),
        ));
    }
}
/// Pages are leased exclusively until the recording submits or is abandoned.
/// Submitted pages can be reused: subsequent queue writes are ordered after the
/// earlier submission. Never upload a page before its owning recording finishes.
#[derive(Debug, Default)]
pub(crate) struct DrawUploads {
    pages: Vec<Page>,
    leases: Vec<Arc<dyn ResourceLease>>,
    lease_ids: Option<HashSet<usize>>,
    // Counts this recording toward idle page expiry when it ends.
    pool: Option<UploadPool>,
}
impl Drop for DrawUploads {
    fn drop(&mut self) {
        self.pages.clear();
        if let Some(pool) = &self.pool {
            pool.lock()
                .unwrap_or_else(|e| e.into_inner())
                .finish_recording();
        }
    }
}
pub(crate) trait ResourceLease: std::fmt::Debug + Send + Sync {}
impl<T: std::fmt::Debug + Send + Sync> ResourceLease for T {}
impl DrawUploads {
    pub(crate) fn new(pool: &UploadPool) -> Self {
        let mut uploads = Self::default();
        uploads.pool = Some(pool.clone());
        uploads
    }
    /// Retains `resource` unless it was the last one retained, without touching
    /// its reference count in that case.
    pub(crate) fn retain_ref<T: ResourceLease + 'static>(&mut self, resource: &Arc<T>) {
        if self
            .leases
            .last()
            .is_some_and(|r| std::ptr::addr_eq(Arc::as_ptr(r), Arc::as_ptr(resource)))
        {
            return;
        }
        self.retain(resource.clone());
    }
    pub(crate) fn retain(&mut self, resource: Arc<dyn ResourceLease>) {
        // Small passes need no hash allocation. Larger collections must avoid a
        // quadratic scan when each label retains independent geometry.
        if self
            .leases
            .last()
            .is_some_and(|r| Arc::ptr_eq(r, &resource))
        {
            return;
        }
        if self.lease_ids.is_none() && self.leases.len() >= 8 {
            self.lease_ids = Some(
                self.leases
                    .iter()
                    .map(|r| Arc::as_ptr(r) as *const () as usize)
                    .collect(),
            );
        }
        let fresh = if let Some(ids) = &mut self.lease_ids {
            ids.insert(Arc::as_ptr(&resource) as *const () as usize)
        } else {
            !self.leases.iter().any(|r| Arc::ptr_eq(r, &resource))
        };
        if fresh {
            self.leases.push(resource);
        }
    }
    pub(crate) fn retain_until_complete(&mut self, queue: &wgpu::Queue) {
        if !self.leases.is_empty() {
            let leases = std::mem::take(&mut self.leases);
            self.lease_ids = None;
            queue.on_submitted_work_done(move || drop(leases));
        }
    }
    pub(crate) fn append(
        &mut self,
        graphics: &crate::GraphicsContext,
        bytes: &[u8],
        alignment: u64,
    ) -> (wgpu::Buffer, std::ops::Range<u64>) {
        let length = bytes.len() as u64;
        if self.pages.last().is_none_or(|p| {
            (p.bytes.len() as u64).div_ceil(alignment) * alignment + length > p.buffer.size()
        }) {
            let mut pool = graphics
                .upload_pool
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let (buffer, mut storage, clip_bindings) = pool.take(length).unwrap_or_else(|| {
                pool.created += 1;
                let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some("RXUI drawing draw upload page"),
                    size: length
                        .max(64 * 1024)
                        .min(graphics.device().limits().max_buffer_size),
                    usage: wgpu::BufferUsages::VERTEX
                        | wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bytes = Vec::with_capacity(buffer.size() as usize);
                (buffer, bytes, Vec::new())
            });
            storage.clear();
            self.pages.push(Page {
                bytes: storage,
                clip_bindings,
                buffer,
                pool: graphics.upload_pool.clone(),
            });
        }
        let page = self.pages.last_mut().unwrap();
        // Renderers with different instance strides can share a page. Callers
        // using first_instance offsets must align starts to their record stride.
        let start = (page.bytes.len() as u64).div_ceil(alignment) * alignment;
        page.bytes.resize(start as usize, 0);
        page.bytes.extend_from_slice(bytes);
        (page.buffer.clone(), start..start + length)
    }
    // Cache immutable bindings with their leased page. The binding range never
    // changes; queue ordering protects the contents just like other page uploads.
    // A hard bound avoids accumulating bindings for every historical mixed stride.
    pub(crate) fn append_clips(
        &mut self,
        graphics: &crate::GraphicsContext,
        bytes: &[u8],
        alignment: u64,
    ) -> wgpu::BindGroup {
        let (buffer, range) = self.append(graphics, bytes, alignment);
        let page = self.pages.last_mut().unwrap();
        if let Some((_, group)) = page
            .clip_bindings
            .iter()
            .find(|(offset, _)| *offset == range.start)
        {
            return group.clone();
        }
        let group = graphics.clip_group(Some((&buffer, range.start)));
        graphics
            .upload_pool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clip_bindings_created += 1;
        if page.clip_bindings.len() < 64 {
            page.clip_bindings.push((range.start, group.clone()));
        }
        group
    }
    pub(crate) fn upload(&self, queue: &wgpu::Queue) {
        profiling::scope!("rxui_draw::upload_instances");
        for page in &self.pages {
            queue.write_buffer(&page.buffer, 0, &page.bytes);
        }
    }
}

/// Logical upload-pool measurements shared by clones of a context.
///
/// Counts library-owned pages and cached clip bindings, not process RSS, driver
/// allocations, physical GPU memory, or resources retained internally by wgpu.
/// Counters are cumulative for this device; idle gauges exclude active recordings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadStats {
    /// Pages allocated since context creation.
    pub pages_created: u64,
    /// Page leases satisfied from the idle pool.
    pub pages_reused: u64,
    /// Idle pages released by the 120-recording expiry policy.
    pub pages_released: u64,
    /// Clip bindings created for upload pages, including uncached overflow.
    pub clip_bindings_created: u64,
    /// Pages currently retained in the idle pool.
    pub idle_pages: usize,
    /// Sum of idle page GPU buffer sizes in bytes (logical allocation sizes).
    pub idle_buffer_bytes: u64,
    /// Sum of idle CPU staging vector capacities in bytes.
    pub idle_staging_capacity_bytes: usize,
    /// Clip bindings retained by idle pages; at most 64 per page.
    pub idle_clip_bindings: usize,
}
impl crate::GraphicsContext {
    /// Snapshots upload counters without polling or waiting for GPU completion.
    /// Submitted pages return at submit because managed queue writes are ordered;
    /// abandoned recordings return on drop. Surplus pages and their clip bindings
    /// expire after 120 later recordings. This is resource accounting, not a
    /// measurement of physical memory reclamation by the backend.
    pub fn upload_stats(&self) -> UploadStats {
        let pool = self.upload_pool.lock().unwrap_or_else(|e| e.into_inner());
        UploadStats {
            pages_created: pool.created,
            pages_reused: pool.reused,
            pages_released: pool.released,
            clip_bindings_created: pool.clip_bindings_created,
            idle_pages: pool.pages.len(),
            idle_buffer_bytes: pool.pages.iter().map(|(buffer, _)| buffer.size()).sum(),
            idle_staging_capacity_bytes: pool.pages.iter().map(|(_, bytes)| bytes.capacity()).sum(),
            idle_clip_bindings: pool.clip_bindings.iter().map(Vec::len).sum(),
        }
    }
}
