use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Weak},
};

/// A snapshot of pipelines retained by a renderer's material cache.
///
/// Counts describe cache handles, not total GPU allocations: prepared scopes,
/// recordings, command buffers, and the GPU may retain evicted pipelines longer.
/// Inspecting these statistics scans the cache; drawing does not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineCacheStats {
    /// Total cached pipeline variants, including variants of dropped materials.
    pub entries: usize,
    /// Variants whose material still has at least one live clone.
    pub live_entries: usize,
    /// Variants eligible for pruning because all material clones were dropped.
    pub stale_entries: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct CachedPipeline {
    pipeline: wgpu::RenderPipeline,
    // Observe material clones without retaining their shaders/layouts or requiring
    // per-draw bookkeeping. Draw scopes own pipeline handles independently.
    material: Weak<()>,
    // Only a successful renderer-owned validation scope proves this pipeline valid.
    validated: bool,
}

impl CachedPipeline {
    pub(crate) fn new(pipeline: wgpu::RenderPipeline, material: &Arc<()>) -> Self {
        Self {
            pipeline,
            material: Arc::downgrade(material),
            validated: false,
        }
    }

    pub(crate) fn is_validated(&self) -> bool {
        self.validated
    }

    /// Call only after pipeline creation and its validation scope both succeed.
    pub(crate) fn mark_validated(&mut self) {
        self.validated = true;
    }
}

impl std::ops::Deref for CachedPipeline {
    type Target = wgpu::RenderPipeline;
    fn deref(&self) -> &Self::Target {
        &self.pipeline
    }
}

impl PartialEq for CachedPipeline {
    fn eq(&self, other: &Self) -> bool {
        self.pipeline == other.pipeline
    }
}

pub(crate) fn stats<K>(cache: &HashMap<K, CachedPipeline>) -> PipelineCacheStats {
    let live_entries = cache
        .values()
        .filter(|p| p.material.strong_count() != 0)
        .count();
    PipelineCacheStats {
        entries: cache.len(),
        live_entries,
        stale_entries: cache.len() - live_entries,
    }
}

pub(crate) fn prune<K: Eq + Hash>(cache: &mut HashMap<K, CachedPipeline>) -> usize {
    let before = cache.len();
    cache.retain(|_, pipeline| pipeline.material.strong_count() != 0);
    cache.shrink_to_fit();
    before - cache.len()
}

impl PartialEq<wgpu::RenderPipeline> for CachedPipeline {
    fn eq(&self, other: &wgpu::RenderPipeline) -> bool {
        &self.pipeline == other
    }
}
