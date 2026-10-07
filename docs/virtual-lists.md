# Fixed-height virtual lists

`virtual_list(count, row_height, &handle, cx, render_row)` creates a vertical scroll
area with a persistent scrollbar gutter. It reads placement-specific ScrollHandle
metrics and calls the row builder only for viewport rows plus overscan (two rows
on each side by default). Row closures can borrow application data and the view
context. Descriptions are converted synchronously; painting does not invoke them.

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
Sharing a handle across windows preserves each placement's separate viewport and
offset. A handle can bind only once within one Ui.

Overlapping keyed rows keep identity, focus, local editor state and prepared
resources. Rows leaving overscan unmount; removed focus/capture is released and
stale semantic identities are rejected. Scrolling back creates a new placement for
that row. Persistent row state, selection and data belong in application entities.
Offscreen editors are not pinned and pending capture is not kept indefinitely.
This first version does not implement variable heights, virtual tables, automatic
keyboard navigation to unmounted rows, or scroll-anchor preservation when items
are inserted above the viewport.

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
cargo run -p rxui --example workspace_window --features native --locked
cargo test -p rxui --all-features virtual_list_tests --locked
cargo bench -p rxui --bench virtual_list --locked
```
