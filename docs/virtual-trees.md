# Controlled virtual trees

`virtual_tree` is a reusable fixed-height, single-column tree. `TreeData` stores
stable `Key` identities, parent keys, names and explicit leaf/branch flags. Build
it once from parent-before-child data; sibling order follows input order. It
validates duplicate keys across **all** domain nodes, including collapsed ones,
and rejects missing/later/leaf parents and cycles. Empty branches remain branches;
this slice has no lazy loading state, requests or I/O.

`TreeState` owns accepted expansion, selection, logical activity and range anchor.
The application owns this value; `TreeProposal` is a controlled suggestion. Accept
with `proposal.accept(&mut state, &scroll, row_height, cx)` inside the listener to
apply and reveal the active key in the source placement. Explicit disclosure
commands reveal only if collapse repairs hidden activity; expanding a different
branch does not scroll back to an unrelated active row. Ignore proposals to
reject them. `state.apply(proposal)` accepts without scrolling; `state.reveal(key,
&scroll, height, cx)` independently reveals a visible key, returning false for
hidden/deleted keys. Stale proposals compare exact accepted snapshot identities,
including hierarchy replacements, so an old range cannot apply after reordering.
There is no optimistic selection, expansion or scroll on rejection.

```rust
use rxui::prelude::*;
struct Files { state: TreeState, scroll: ScrollHandle }
impl View for Files {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let change = cx.listener(|s, p: &TreeProposal, cx| {
            p.accept(&mut s.state, &s.scroll, 24., cx).unwrap();
        });
        virtual_tree(&self.state, 24., &self.scroll, cx, change, |_, node, _| {
            label(node.label.clone())
        }).accessibility_label("Files").fill_width().height(320.)
    }
}
```

`set_data`, `set_expanded`, and `set_selection` support application updates. A data
or expansion change rebuilds the visible preorder in O(visible) time, with hash
set cleanup proportional to retained expanded/selected keys. Scrolling reuses
that projection; only viewport rows and two overscan rows per side are rendered.
Data validation is O(all domain nodes). Navigation uses a cached key-to-visible
index map. State proposals copy selected/expanded sets; type-ahead may scan all
visible names. There is no element per unmounted row. Geometry and the 2^24-pixel
extent limit match `virtual_list`; rows must have a finite positive fixed height.

Logical activity is separate from selection. Up/Down, Home/End and PageUp/PageDown
move through visible preorder. Right expands a branch, then moves to its first
child; Left collapses a branch, then moves to its parent. Plain navigation selects
its destination. Primary-modifier navigation changes activity only. Space or
pointer selection with the platform primary modifier toggles in multiple mode;
Shift selects the inclusive visible anchor range, and Primary+Shift adds it.
Single mode always keeps at most one selected key. Shifted uppercase type-ahead
behaves like lowercase input, without range extension. Alt/Option and the
non-primary Control/Command modifier leave keyboard shortcuts unhandled for the
application. Unicode lowercase prefix
search uses actual monotonic input time (700 ms timeout), with repeated
single characters cycling matches. Extending a prefix keeps the current match;
an invalid extended prefix falls back to the newest character. No task feature or browser-unsupported wall
clock is required. The widget retains a monotonic clock origin in accepted state
across descriptions (`web_time::Instant` on wasm, `std::time::Instant` elsewhere),
so prefixes expire during idle periods without frame updates. The pure
`TreeState::propose(..., time)` API still takes deterministic caller-supplied
`Duration` values for custom command dispatch and tests. Use `state.input_time()`
for custom adapters mixed with widget input so both share the same time base.

Collapse preserves selected descendants and hidden anchors, using full domain
membership. Hidden activity moves to its closest visible ancestor. A hidden
anchor cannot define a visible range: Shift falls back to the previous active
row, preserving the hidden anchor for later expansion. Deleted selections and
anchors are removed. Deleted activity falls back to the first root. An empty
tree has no active key. Replacing data/expansion invalidates outstanding proposals.
Share immutable `TreeData` between independent `TreeState`s for independent
expansion and selection; a shared state has shared logical selection/activity,
while `ScrollHandle` metrics and reveal commands remain placement-local.

## Focus and accessibility boundary

Render **passive labels and decoration only**. Do not render buttons, text inputs,
focusable elements, or children with input listeners inside a row. This is a
renderer contract, not an automatic sanitizer. The tree owns row input and a
persistent focusable root; individual rows are not Tab stops. Put interactive
controls outside the tree. A primary pointer press selects a row and focuses the
root. Rows are `Element::list_row`s with the theme's `row_padding_x` on each side.
Inside it, the tree supplies 16-pixel indentation per level and a 20-pixel
disclosure before custom content, centering both vertically in the row; pass the
theme's `row_height` (24 balanced, 22 compact) as the row height to match other
lists. A primary disclosure press expands/collapses a
branch without changing selection; other row presses select, respecting modifiers.
Expansion is also available through Right/Left and semantic actions. Built-in
scrollbars retain pointer input, with keyboard focus and scrolling on the root.

Mounted rows expose TreeItem roles, names, selected/expanded states, hierarchy
levels and sibling set metadata, under a Tree root which exposes its multiple
selection policy. Name the root using `.accessibility_label("Files")`. Leaves omit expanded state.
There are no incorrect List/ListItem wrappers. Active-descendant resolves only
while its keyed row is mounted; manual scrolling can leave logical activity
unmounted and then omits that relationship. Expand, Collapse and collection
Select actions dispatch controlled proposals on mounted rows; text SetSelection
retains its existing meaning. AccessKit advertises expansion actions and maps
TreeItem Click to collection Select. This is **not** a full virtual accessibility
provider: unmounted rows have no semantic identity/actions. Native screen-reader
navigation of the full unmounted hierarchy is not claimed.

Run the standalone hierarchical example with
`cargo run -p rxui --features native --example tree_panel_window`.
It contains 100 folders and 100,000 leaves. Measured heights, tables, grids,
2D virtualization and async lazy children are outside this slice.
