# Virtual lists

`virtual_list(count, row_height, &handle, cx, render_row)` creates a vertical scroll
area with a persistent scrollbar gutter. It reads placement-specific ScrollHandle
metrics and calls the row builder only for viewport rows plus overscan (two rows
on each side by default). Row closures can borrow application data and the view
context. Descriptions are converted synchronously; painting does not invoke them.
`variable_list(&rows, &handle, cx, render_row)` does the same for rows of differing
heights; see [variable heights](#variable-heights).

```rust
struct Files { scroll: ScrollHandle, names: Vec<String>, selected: usize }
impl View for Files {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        virtual_list(self.names.len(), 32., &self.scroll, cx, |index| {
            button(self.names[index].clone()).key(self.names[index].clone())
                .on_click(cx.listener(move |this, _, _| this.selected = index))
        }).overscan(3).height(400.)
    }
}
```

Use `rxui::prelude::*` for these imports. Supply unique stable data keys when the
items can reorder or change; absent explicit keys, row indices are used. A fixed
slot includes all row spacing; draw padding within it rather than adding list gaps.
Rows fill the slot and clip overflowing content. Give the list bounded height via
its parent or `.height(...)`, just as for a normal scroll area. `.scrollbars(false)`
hides the gutter while preserving wheel/programmatic scrolling.

Fixed-height rows do not measure their content to choose a height. Wrapped captions
can therefore be taller than the row slot and get clipped. Use
`.text_overflow(TextOverflow::Ellipsis)` on row buttons to show a single line,
including when a label contains paragraph breaks. Measurement and painting use the
same grapheme-safe truncation; semantics retain the full label. Inputs and
selectable labels preserve their editing geometry instead of truncating. Custom
`TextMeasure` implementations should honor `request.style.overflow` as the built-in
`UiPainter` does.

For trees, indent only the leading side with `.padding_left(...)`, leaving a small
constant `.padding_right(...)`. Cap indentation relative to
`handle.state(cx).viewport[0]` so deep nodes retain usable label space after a resize.
Keep selection and expansion on the full-width button, rather than an indented
wrapper, to preserve the row's hit target. Do not use symmetric `.padding_x(depth)`
for indentation. The native `tree_panel_window` example demonstrates narrow rows
with long labels and deep indentation:

```sh
cargo run -p rxui --example tree_panel_window --features native --locked
```

Initial preparation publishes viewport metrics, then describes the visible range
in the existing bounded preparation loop. Resizes, count changes, offset clamping
and large jumps settle before painting. The virtual content reserves the complete
fixed-height extent regardless of which rows are mounted. Invalid height (zero,
negative or nonfinite) and a total extent beyond 2^24 logical pixels return
UiError::InvalidStyle without invoking the row closure. The limit keeps f32 layout
within single-pixel precision; extremely large coordinate spaces need a different
origin/rebasing design.

`handle.reveal_row(cx, index, row_height)` queues the smallest movement needed to
show a row in the listener's source placement. A ScrollPlacement exposes the same
operation for explicit weak sources. The caller owns the data index; offsets clamp
to current content geometry. These commands do not create a row before preparation.
`handle.reveal_span(cx, start, extent)` is the same operation for any vertical
content span. Sharing a handle across windows preserves each placement's separate
viewport and offset. A handle can bind only once within one Ui.

Reveals and mouse-wheel steps follow the list's `.scroll_behavior(...)`. With
`ScrollBehavior::Smooth` (the default on macOS and Windows) they ease to the new
offset, and each frame mounts the rows of the offset it displays, so a long reveal
mounts a viewport of rows per frame rather than every row in between. `jump_to`
moves without animation; see [smooth scrolling](animation.md#smooth-scrolling).

Overlapping keyed rows keep identity, focus, local editor state and prepared
resources. Rows leaving overscan unmount; removed focus/capture is released and
stale semantic identities are rejected. Scrolling back creates a new placement for
that row. Persistent row state, selection and data belong in application entities.
Offscreen editors are not pinned and pending capture is not kept indefinitely.
Virtual tables and automatic keyboard navigation to unmounted rows are not
implemented: keep the selection in application state and call a reveal method when
it moves.

## Variable heights

`VirtualRows` holds a stable key and a logical height per row. Keep it in
application state beside the row data and change both together:

```rust
struct Results { scroll: ScrollHandle, rows: VirtualRows, hits: Vec<Hit> }
impl View for Results {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        variable_list(&self.rows, &self.scroll, cx, |index| {
            let hit = &self.hits[index];
            if hit.is_file { label(hit.path.clone()) } else { label(hit.line.clone()) }
        }).fill_height()
    }
}
// When a file's matches arrive:
// this.rows.splice(at..at, matches.iter().map(|m| (m.id, 22.)));
// this.hits.splice(at..at, matches);
```

Row `index` occupies `rows.height(index)` logical pixels, including spacing, and is
clipped to that slot. Its slot is keyed by `rows.key(index)`, so a mounted row keeps
its identity, focus and local state when rows are inserted, removed or reordered
around it; a key set on the returned row element stays on that element inside the
slot. Keys must be unique among the rows.

A Fenwick (binary indexed) tree over the heights answers `offset(index)`,
`index_at(offset)` and `total()` in O(log n). `set_height` and `push` are O(log n);
`insert`, `remove`, `splice` and collecting are O(n). `index_of(key)` uses a key
map that the first lookup after an insertion or removal rebuilds in O(n). Describing
the list finds the first and last visible rows with two O(log n) lookups, then sums
the heights of the mounted rows. Heights are summed in f64. The extent rules match
the fixed list: a nonfinite, zero or negative height, or a total beyond 2^24 logical
pixels, makes preparation fail with UiError::InvalidStyle before any row is
described; `rows.is_valid()` reports whether the list would accept the rows. Such
heights are stored and count as zero in offsets until corrected.

**Scroll anchoring.** When the rows change, the list keeps the first visible row
where it was on screen. Each description of a placement records the keys and tops
of up to eight rows from the viewport top. When a later description sees changed
rows, it finds the first recorded key that still exists and moves the scroll offset
by however far that row's top moved. Rows inserted or removed above, and heights
changed above, therefore leave the content under the viewport still; changes to the
anchor row's own height and below it do not move the offset. If the anchor row was
removed, the next recorded row that survives is kept in place. Finding the key is
O(1) when rows changed only below, or only by insertions or removals above;
otherwise it uses `index_of`. The correction is an ordinary queued scroll command
applied in the same preparation, so nothing paints at the intermediate offset, and
the result clamps to the new extent. It is never animated; a smooth scroll in
flight moves along with it and still ends on the content it was heading to.

Anchoring is skipped:

- while the viewport is at offset zero, so rows inserted at the start stay visible,
  as with CSS scroll anchoring;
- when a scroll command for the viewport was queued in the same update, such as
  `reveal_list_row` after inserting rows; the explicit command wins;
- when no recorded row survives, for example after replacing every key;
- with `.anchoring(false)`. Turning it back on records a fresh anchor rather than
  replaying earlier changes.

`handle.reveal_list_row(cx, &rows, index)` queues the smallest movement that shows
the row, as `reveal_row` does for fixed heights, in O(log n). An index past the end
or rows the list would reject return `ScrollError::InvalidOffset`.

Heights are supplied by the caller rather than measured. Measuring rows after
layout would need a second layout pass whenever a newly mounted row's estimate was
wrong, and a large jump can mount a whole viewport of estimated rows; the
preparation loop is bounded to four passes. The measurements would also have to live
per placement, because `VirtualRows` is application state shared by windows, which
would let two windows disagree about offsets. An application that needs content
heights, such as wrapped text, computes them with its own measurer and calls
`set_height`; anchoring keeps the viewport still while such corrections arrive.

## Accessibility and costs

The mounted List container publishes the complete set size; each ListItem has a
zero-based position. Unmounted rows have no assistive Focus/ScrollIntoView target.
Viewport/scrollbar actions can expose another range. A complete indexed screen-
reader navigation provider remains future work; see [semantics](semantics.md).

Scroll metric changes reevaluate the subscribed view. Only the visible range is
described, but unrelated siblings in that same view also rebuild. Place a large
list in a child Entity<View> to contain that description work. Within a stable row
range, layout and text measurement are retained. Crossing a row mounts/unmounts
only a bounded number of rows. Glyph preparation for newly exposed text still costs
work; virtualization reduces the amount of text prepared at once, not that cost
per new glyph. [Scaling measurements](performance/virtual-lists.md) distinguish
mock-layout CPU costs from native rendering latency.

```sh
cargo run -p rxui --example virtual_list_window --features native --locked
cargo run -p rxui --example variable_list_window --features native --locked
cargo run -p rxui --example workspace_window --features native --locked
cargo test -p rxui --all-features virtual --locked
cargo bench -p rxui --bench virtual_list --locked
```
