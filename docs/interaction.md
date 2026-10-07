# Input, scrolling and split panes

RXUI keeps application data in entities and interaction in each retained UI
placement. These controls use the same `cx.listener(...)` bindings as buttons and
text inputs. A listener receives its owner's current mutable state and a typed
context; no central message enum or separate input dispatcher is required.

Try the standalone [workspace example](../crates/rxui/examples/workspace_window.rs):

```sh
cargo run -p rxui --example workspace_window --features native --locked
```

It includes a controlled sidebar, a nested vertical split, two scroll areas,
programmatic scrolling, an editable name and a captured draggable card. A second
window shares pane sizes, selection and card position, while the framework keeps
scroll, capture, hover and focus separate. Custom interaction values stored in the
shared entity still follow ordinary shared-state semantics.

## Routed listeners

Elements expose `on_pointer_down`, `on_pointer_move`, `on_pointer_up` and
`on_pointer_cancel`, plus corresponding `_capture` builders. `on_key_down` and
`on_key_up` also have capture variants. Ordinary callbacks run from the target up
to its ancestors; capture callbacks run from the root down to the target first.
The target's callbacks report `EventPhase::Target` in either traversal. Passive
children can target an ancestor with a pointer listener. Bounds, clipping,
sibling paint order, disabled state, inertness and pointer policies still apply.

```rust
column().size(180., 100.).focusable(true).cursor(Cursor::Grab)
    .on_pointer_down(cx.listener(|this, event: &PointerInput, _| {
        if event.button == Some(PointerButton::Primary) {
            this.drag_origin = this.position;
            event.capture_pointer();
            event.focus();
            event.prevent_default();
        }
    }))
    .on_pointer_move(cx.listener(|this, event: &PointerInput, _| {
        if let Some(delta) = event.drag_delta() {
            this.position = [this.drag_origin[0] + delta[0],
                             this.drag_origin[1] + delta[1]];
        }
    }))
```

`PointerInput` carries logical window position, position relative to the executing
element, current border/parent-content bounds, modifiers, changed/held buttons,
and the original captured position/bounds. `target` is the routed target;
`current_target` is the executing listener; `hit_target` independently reports the
element under the pointer. Keyboard callbacks carry logical key identity,
pressed/released state, repeat and modifiers. Character keys are distinct from
IME/committed text insertion.

Requests apply after each callback. `stop_propagation()` ends the remaining route
without cancelling the default action. `prevent_default()` cancels that dispatch's
control default without ending propagation. Capturing or requesting focus on an
ancestor does not automatically override the descendant's default; prevent it
when taking ownership of the interaction. `.focusable(true)` includes a custom
container in sequential/accessible focus without creating button or editing
behavior. `.cursor(...)` applies under the pointer; the capture owner's cursor
wins during a drag.

`capture_pointer()` routes subsequent motion and release to the requesting
element while a mouse button is held. Matching release ends capture automatically;
`release_pointer()` can end it explicitly. Leaving the viewport preserves capture.
Escape, host cancellation/deactivation, removal, hiding, inertness, changed control
axis and unavailable pointer input cancel it. Removal snapshots the old cancellation
route so surviving owner/ancestor listeners can still clean up before the next
prepared tree is exposed. A removed component's expired listener remains inert.
`cancel_reason` distinguishes Escape, Host and TargetUnavailable.

Capture is per UI and covers one mouse gesture with primary, secondary or middle
buttons. It routes events delivered by the host; it is not an OS-wide drag session,
touch/pen API, drag-and-drop data transfer, or cross-window capture service.

## Scroll references and visible controls

Existing `.scroll_y()`, `.scroll_x()` and `.scroll()` builders continue to provide
retained scrolling without a visible bar. Attach `.scroll_handle(handle.clone())`
to a scroll-enabled viewport for commands/metrics, or use a stock scroll area:

```rust
scroll_area(column().children(rows))
    .handle(self.rows.clone())
    .axes(ScrollAxes::Vertical)
    .fill_width().height(320.)
```

`scroll_area(content)` defaults to vertical scrolling and a 12-unit persistent
scrollbar gutter. `.axes(ScrollAxes::Horizontal | Both)`, `.scrollbars(false)` and
`.scrollbar_size(...)` select the presentation. It needs bounded parent space;
its default flex growth/basis and zero minima allow it to occupy the remaining
space below a fixed header. Gutters stay reserved when content fits, avoiding
layout jumps. `scrollbar(handle, axis)` can also be placed separately in the same
UI. Unbound/zero-range bars are disabled, omit focus and resize actions, and keep
painting their track/thumb where geometry is available.

Bars support captured thumb dragging, clicking the track to center the thumb,
axis arrows, PageUp/PageDown, Home/End and accessible numeric actions. Wheel input
over the bar routes to its bound viewport. Escape restores the initial offset;
other cancellation ends the gesture at its current offset. Offsets clamp when
content or the viewport changes. Ordinary wheel motion still chains unused motion
through the viewport's scroll ancestors.

`ScrollHandle` is a cloneable placement reference, not a shared offset value. Bind
one handle to at most one viewport per UI. Duplicate bindings or a handle attached
to a non-scroll element produce `UiError`. Sharing a handle across windows is
supported: a listener/view context resolves the handle in its originating UI.
Initialization and ordinary application updates have no implicit placement.

```rust
button("Top").on_click(cx.listener(|this, _, cx| {
    this.rows.scroll_to(cx, [0., 0.]).expect("live viewport");
}))
```

`state(cx)` returns `Option<ScrollState>` with logical offsets, ranges, viewport
size and bounds. It is absent before the first layout or after removal. Reading
it in a `ViewContext` subscribes that mount to metrics; normal successful evaluation
replaces the read set, so stopping the read unsubscribes it. Listener/update reads
are snapshots and do not subscribe. First layout and queued commands can require
another evaluation before preparation settles. Four settling passes diagnose a
layout/metric feedback loop with `UnstableControlLayout`, leaving geometry unavailable
until a corrected retry.

Capture an explicit `ScrollPlacement` **inside a listener** when later work needs
that exact viewport:

```rust
let placement = this.rows.placement(cx)?;
// Capture placement in a task completion or app command:
placement.scroll_by(cx, [0., 120.])?;
```

An explicit placement resolves outside event dispatch, checks runtime identity,
and does not keep a removed viewport/window alive. Removal/replacement/closure
makes it `Disposed`. Commands queue until UI preparation, apply in order against
current geometry, and clamp then. A stale queued target cannot scroll its replacement.
Nonfinite offsets/deltas are rejected. `needs_prepare()` includes queued commands.

## Controlled splits

```rust
split_row(sidebar, editor)
    .position(self.sidebar)
    .min_first(160.).min_second(400.)
    .divider_size(8.)
    .on_resize(cx.listener(|this, event: &ResizeEvent, _| {
        this.sidebar = event.position;
    }))
```

`split_row` changes the first pane's width; `split_column` changes its height.
They can nest and size naturally in bounded flex space. `SplitPosition::Fraction`
is a fraction of the space available for both panes, excluding the divider;
`Pixels` requests a logical first-pane size. Both pane minima constrain the
actual allocation. An oversized pixel request stays in application state so it
can be honored again when the window grows; merely pressing/releasing the divider
does not replace it with the current constrained size. Below the sum of the minima,
panes retain their minima, the split clips overflow, and the divider is read-only.

`ResizeEvent` proposes `position`, actual `first_size`, `available` space and a
`ResizePhase`: Begin, Drag, End, Cancel, Keyboard or Accessibility. The application
can accept, normalize or reject a proposal. Dragging retains the configured
fraction/pixels mode. Escape's Cancel restores the configured position at press;
host/removal cancellation ends at the current application position. Without an
`on_resize` listener the divider is read-only.

Dividers have a full hit strip and a narrower painted line. They use the resize
cursor, sequential focus, axis arrows in 8-unit increments and Home/End. Semantic
snapshots expose logical numeric values/min/max/step; AccessKit uses ScrollBar and
Splitter roles, numeric SetValue/Increment/Decrement actions, and physical separator
orientation for splits. These actions use the same retained/controlled update path.
Native AX actions and pointer dragging were exercised on macOS; full VoiceOver
navigation/announcement usability is not established by those checks.

## Hosting and performance boundaries

Application forwards logical mouse/keyboard events, modifiers, cancellation,
native cursors and accessible range actions. Custom hosts prepare geometry before
input, route pointer events through `pointer_with_text` when using shaped editing,
and call `Ui::key`. Honor its `default_prevented` result before applying ordinary
Tab, button or text defaults. Ui handles range keys and capture Escape itself.
Forward `PointerEvent::Cancelled` on platform/device cancellation or deactivation;
`set_active(false)` also queues cancellation callbacks for the next preparation.
`needs_prepare()` includes that pending work. Dispose a placement's GPU caches
with `painter.forget(&ui)` and drop the UI when removing it.

Pointer payloads and warmed routing buffers avoid per-motion payload/path allocation;
optional listener configuration is boxed only on elements that use it. Listener
callbacks still conservatively invalidate their owner. Use child entities to keep
large pane descriptions outside a frequently resized parent. Stock scrollbar
motion has no application callback and changes geometry without view evaluation,
text measurement or Taffy layout unless a view explicitly reads metrics. Split
resizing changes layout and may reflow/measure text; retaining a pane description
does not remove that work.

All rows remain retained and scrolling refreshes tree geometry. There is no
virtualization or constant-cost large-list guarantee. See the
[CPU measurements](performance/interaction.md) for reproducible workloads and limits.
Focus scopes/tab selection/focus restoration, drag overlays/docking trees and
cross-native-window detach remain following stages; these primitives supply their
input, scroll and pane-sizing foundation.
