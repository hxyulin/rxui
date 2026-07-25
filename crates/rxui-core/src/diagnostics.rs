//! View-layer instrumentation for update-model work.
//!
//! The retained engine's own `PassStats` measures layout, composition, paint,
//! and accessibility work *below* the component boundary. It cannot see the
//! dominant cost of an interaction in today's framework: constructing the root
//! [`crate::View`] tree and diffing it against the retained shadow tree. The
//! counters here close that gap.
//!
//! Counters accumulate in a thread-local and are read with [`ViewStats::take`]
//! (snapshot and reset) or [`ViewStats::current`] (peek). Because view
//! construction and reconciliation always run on the thread that owns the
//! retained root, thread-local accumulation needs no synchronization, and
//! parallel test threads never observe each other's work.
//!
//! Recording is unconditional: it is compiled into the library on every
//! profile, with no `#[cfg(test)]` gating, so integration tests that link the
//! library without `cfg(test)` observe exactly the same counters as a real
//! application.
//!
//! # Example
//!
//! ```
//! use rxui_core::diagnostics::ViewStats;
//!
//! let before = ViewStats::take();
//! assert_eq!(before.nodes_built, ViewStats::current().nodes_built);
//! ```

use std::cell::Cell;

/// Counters describing view construction and shadow-tree reconciliation work.
///
/// Every field counts occurrences since the last [`ViewStats::take`] on the
/// current thread. All counters are deterministic: they contain no timing and
/// no sampling, so a test may assert exact equality against a recorded
/// baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewStats {
    /// Calls to a user [`crate::Component::view`] or
    /// [`crate::ComponentWithProps`] implementation.
    ///
    /// Counts both the root view build performed by
    /// [`crate::ComponentRuntime`] and every nested component's own view
    /// build. A single interaction that rebuilds a nested component and then
    /// re-renders the root therefore counts more than once, which is exactly
    /// the waste this instrument exists to expose.
    pub component_views: usize,

    /// View nodes mounted from scratch.
    ///
    /// One increment per view node whose retained state is created rather than
    /// reconciled. "View node" means one node of the lightweight view tree,
    /// including modifier wrappers such as `enabled`, `visible`, `frame`, and
    /// `map_action`, which reuse their child's retained node but still own
    /// reconciliation state of their own.
    pub nodes_built: usize,

    /// View nodes reconciled in place against a previous instance.
    ///
    /// The counterpart of [`Self::nodes_built`], counted over the same set of
    /// view nodes. A node is counted here when its previous mounted instance
    /// had a matching kind and could be updated instead of replaced.
    pub nodes_rebuilt: usize,

    /// Child-list reconciliation passes.
    ///
    /// One increment per container (flex, stack, scroll, split pane) whose
    /// child list is reconciled during a rebuild, covering both the keyed and
    /// the positional strategy. Mounting a container from scratch does not
    /// count here, because its children are appended rather than reconciled.
    pub containers_reconciled: usize,

    /// Calls that hand a new child list to the engine.
    ///
    /// Today this tracks [`Self::containers_reconciled`] one-for-one, because
    /// child-list reconciliation always republishes the resulting order. Once
    /// update isolation can prove a child list unchanged, this counter is
    /// expected to fall below `containers_reconciled`.
    pub set_children_calls: usize,

    /// Memoized subtrees skipped without rebuilding.
    ///
    /// No producer exists yet; memoization arrives in a later phase. Expect
    /// this to read 0.
    pub memo_hits: usize,

    /// Memoized subtrees whose memo was invalid and had to be rebuilt.
    ///
    /// No producer exists yet; memoization arrives in a later phase. Expect
    /// this to read 0.
    pub memo_misses: usize,

    /// Virtualized rows created for a newly visible range.
    ///
    /// No producer exists yet; row virtualization arrives in a later phase.
    /// The `virtual_tree` and `virtual_table` views take a caller-supplied
    /// visible range and own no row pool, so they do not realize or recycle
    /// rows. Expect this to read 0.
    pub rows_realized: usize,

    /// Virtualized rows reused for a newly visible range.
    ///
    /// No producer exists yet; row virtualization arrives in a later phase.
    /// Expect this to read 0.
    pub rows_recycled: usize,
}

thread_local! {
    static VIEW_STATS: Cell<ViewStats> = const { Cell::new(ViewStats::new()) };
}

impl ViewStats {
    /// Creates an all-zero snapshot.
    pub const fn new() -> Self {
        Self {
            component_views: 0,
            nodes_built: 0,
            nodes_rebuilt: 0,
            containers_reconciled: 0,
            set_children_calls: 0,
            memo_hits: 0,
            memo_misses: 0,
            rows_realized: 0,
            rows_recycled: 0,
        }
    }

    /// Reads the counters accumulated on this thread without resetting them.
    pub fn current() -> Self {
        VIEW_STATS.try_with(Cell::get).unwrap_or_default()
    }

    /// Reads the counters accumulated on this thread and resets them to zero.
    pub fn take() -> Self {
        VIEW_STATS
            .try_with(|stats| stats.replace(Self::new()))
            .unwrap_or_default()
    }

    /// Measures one unit of work in isolation.
    ///
    /// Resets the thread's counters, runs `work`, and returns its value with
    /// the counters it produced. Prefer this over a manual
    /// [`Self::take`]/[`Self::take`] pair, which is easy to get wrong when the
    /// measured work can return early.
    pub fn measure<T>(work: impl FnOnce() -> T) -> (T, Self) {
        let _ = Self::take();
        let value = work();
        (value, Self::take())
    }

    /// Records one call to a user `view()` implementation.
    pub fn record_component_view() {
        Self::record(|stats| stats.component_views += 1);
    }

    /// Records one view node mounted from scratch.
    pub fn record_node_built() {
        Self::record(|stats| stats.nodes_built += 1);
    }

    /// Records one view node reconciled in place.
    pub fn record_node_rebuilt() {
        Self::record(|stats| stats.nodes_rebuilt += 1);
    }

    /// Records one child-list reconciliation pass.
    pub fn record_container_reconciled() {
        Self::record(|stats| stats.containers_reconciled += 1);
    }

    /// Records one child list handed to the engine.
    pub fn record_set_children() {
        Self::record(|stats| stats.set_children_calls += 1);
    }

    /// Records one memoized subtree skipped without rebuilding.
    pub fn record_memo_hit() {
        Self::record(|stats| stats.memo_hits += 1);
    }

    /// Records one memoized subtree rebuilt after a failed memo check.
    pub fn record_memo_miss() {
        Self::record(|stats| stats.memo_misses += 1);
    }

    /// Records one virtualized row created for a newly visible range.
    pub fn record_row_realized() {
        Self::record(|stats| stats.rows_realized += 1);
    }

    /// Records one virtualized row reused for a newly visible range.
    pub fn record_row_recycled() {
        Self::record(|stats| stats.rows_recycled += 1);
    }

    fn record(update: impl FnOnce(&mut Self)) {
        let _ = VIEW_STATS.try_with(|stats| {
            let mut current = stats.get();
            update(&mut current);
            stats.set(current);
        });
    }
}
