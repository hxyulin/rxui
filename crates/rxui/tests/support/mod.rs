//! Shared helpers for the update-model gate.
//!
//! A later phase moves this into a dedicated test-support crate. Until then it
//! stays a plain module directory so it adds no test target of its own.

use std::hash::{DefaultHasher, Hash, Hasher};

use rxui::{
    Component, ComponentHost, Theme, engine::UiRoot, geometry::LogicalSize, inspect::ViewStats,
};

/// Canonical, identity-free rendering of the engine's semantic snapshot.
///
/// `NodeId`s are deliberately excluded: a freshly mounted tree allocates
/// different arena slots than an incrementally updated one, so raw identities
/// can never match. Everything an assistive client can observe *is* included.
/// Lines are sorted so the comparison is a multiset over semantic nodes; paint
/// order is covered separately by [`fragment_digest`].
pub fn semantic_digest(ui: &UiRoot) -> String {
    let mut lines = ui
        .semantic_snapshot()
        .iter()
        .map(|node| {
            format!(
                "role={:?} label={:?} value={:?} selected={:?} expanded={:?} \
                 bounds={:?} focusable={} focused={} enabled={} actions={:?}",
                node.data.role,
                node.data.label,
                node.data.value,
                node.data.selected,
                node.data.expanded,
                node.bounds,
                node.focusable,
                node.focused,
                node.enabled,
                node.actions,
            )
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines.join("\n")
}

/// Paint-order fragment and geometry digest for the engine's cached scene.
///
/// The scene is collected by a depth-first walk of the retained tree, so this
/// captures paint order, per-fragment world transform, clip, opacity, and
/// (through a hash of the fragment's display list) every paint command and
/// shaped text run. The list contents are hashed rather than embedded so that
/// a mismatch reports the offending fragment index instead of megabytes of
/// display-list debug output.
pub fn fragment_digest(ui: &UiRoot) -> String {
    ui.scene()
        .fragments()
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            let mut hasher = DefaultHasher::new();
            normalize_font_cache_ids(&format!("{:?}", fragment.list)).hash(&mut hasher);
            format!(
                "{index}: transform={:?} clip={:?} opacity={} list={:016x}",
                fragment.transform,
                fragment.clip,
                fragment.opacity,
                hasher.finish(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rewrites per-`FontDatabase` font-cache identities out of a fragment dump.
///
/// A shaped glyph run records `FontFace { cache_id: (collection, face) }`. The
/// collection component is handed out by a process-global counter, one per
/// `FontDatabase`, so a second host shaping identical text produces identical
/// glyphs under a different `cache_id`. Everything else in the dump - commands,
/// brushes, colors, glyph ids, advances, baselines - is content-derived and is
/// compared verbatim.
fn normalize_font_cache_ids(debug: &str) -> String {
    const MARKER: &str = "cache_id: (";
    let mut out = String::with_capacity(debug.len());
    let mut rest = debug;
    while let Some(index) = rest.find(MARKER) {
        let (head, tail) = rest.split_at(index + MARKER.len());
        out.push_str(head);
        out.push('*');
        let comma = tail.find(',').expect("a cache_id tuple has two fields");
        rest = &tail[comma..];
    }
    out.push_str(rest);
    out
}

/// Asserts an incrementally updated tree equals a freshly mounted equivalent.
///
/// This is the anti-cheat guard for the whole gate: no future reduction in
/// [`ViewStats`] may be bought by leaving retained state stale. It mounts a
/// second host from `fresh_state` - which the caller must construct to equal
/// `live`'s final component state - and compares both the semantic snapshot
/// and the fragment/geometry digest.
///
/// `prepare` re-establishes engine-owned transient interaction state on the
/// fresh host (hover, focus, caret, scroll offset), which no amount of
/// component state can reproduce. Pass `|_| {}` when the scenario dispatched
/// only typed actions.
///
/// Call this *after* reading the scenario's counters: mounting the fresh host
/// records view work of its own. The thread's counters are reset on return so
/// the pollution cannot leak into a later measurement.
pub fn assert_incremental_matches_fresh<C: Component>(
    scenario: &str,
    live: &ComponentHost<C>,
    fresh_state: C,
    viewport: LogicalSize,
    theme: Theme,
    prepare: impl FnOnce(&mut ComponentHost<C>),
) {
    let mut fresh =
        ComponentHost::new(fresh_state, viewport, theme).expect("fresh reference host mounts");
    prepare(&mut fresh);
    fresh
        .ui_mut()
        .update_passes()
        .expect("fresh reference host settles");

    assert_digest_eq(
        scenario,
        "semantic tree",
        &semantic_digest(live.ui()),
        &semantic_digest(fresh.ui()),
    );
    assert_digest_eq(
        scenario,
        "fragment/geometry digest",
        &fragment_digest(live.ui()),
        &fragment_digest(fresh.ui()),
    );

    let _ = ViewStats::take();
}

/// Compares two digests and reports the first differing line on failure.
///
/// Digests are long; a raw `assert_eq!` buries the one line that matters in
/// pages of identical output.
fn assert_digest_eq(scenario: &str, what: &str, live: &str, fresh: &str) {
    if live == fresh {
        return;
    }
    let live_lines = live.lines().collect::<Vec<_>>();
    let fresh_lines = fresh.lines().collect::<Vec<_>>();
    let first = live_lines
        .iter()
        .zip(&fresh_lines)
        .position(|(a, b)| a != b)
        .unwrap_or(live_lines.len().min(fresh_lines.len()));
    panic!(
        "{scenario}: incremental {what} diverged from a freshly mounted tree\n\
         lines: incremental={} fresh={}\n\
         first difference at line {first}\n\
         incremental: {}\n\
         fresh      : {}",
        live_lines.len(),
        fresh_lines.len(),
        live_lines.get(first).unwrap_or(&"<missing>"),
        fresh_lines.get(first).unwrap_or(&"<missing>"),
    );
}
