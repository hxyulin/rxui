# Input, scrolling and split panes

RXUI keeps application data in entities and interaction in each retained UI
placement. These controls use the same `cx.listener(...)` bindings as buttons and
text inputs. A listener receives its owner's current mutable state and a typed
context; no central message enum or separate input dispatcher is required.

Try the standalone [workspace example](../crates/rxui/examples/workspace_window.rs):

```sh
cargo run -p rxui --example workspace_window --features native --locked
```

It includes a controlled sidebar, a nested vertical split, two fixed-height virtual lists,
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
IME/committed text insertion, which reaches built-in inputs or a
[text input client](custom-elements.md#text-input-clients) after the key's listeners;
`prevent_default()` on the key-down suppresses that text.

Requests apply after each callback. `stop_propagation()` ends the remaining route
without cancelling the default action. `prevent_default()` cancels that dispatch's
control default without ending propagation. Capturing or requesting focus on an
ancestor does not automatically override the descendant's default; prevent it
when taking ownership of the interaction. `.focusable(true)` includes a custom
container in sequential/accessible focus without creating button or editing
behavior. `.cursor(...)` applies under the pointer; the capture owner's cursor
wins during a drag. The full cursor set, including diagonal resizes, busy states,
drag feedback and `Cursor::Hidden`, is listed in [native feel](native-feel.md#cursors).

`capture_pointer()` routes subsequent motion and release to the requesting
element while a mouse button is held. Matching release ends capture automatically;
`release_pointer()` can end it explicitly. Leaving the viewport preserves capture.
Escape, host cancellation/deactivation, removal, hiding, inertness, changed control
axis and unavailable pointer input cancel it. Removal snapshots the old cancellation
route so surviving owner/ancestor listeners can still clean up before the next
prepared tree is exposed. A removed component's expired listener remains inert.
`cancel_reason` distinguishes Escape, Host and TargetUnavailable.

`click_count` reports consecutive primary presses at one spot (2 for the press
of a double-click, 3 for a triple-click) on the down event, and 1 on every other
event. The native host counts clicks within 500 ms and 4 logical units on the same
target; custom hosts pass their count to `Ui::pointer_with_text_clicks`, while
`Ui::pointer` always reports 1.

`on_hover` delivers a `HoverEvent` with `hovered: true` when the pointer's hit
target moves into the element's subtree and `hovered: false` when it moves out.
Leaves run innermost first, enters outermost first, so a card and its button
both see the pointer arrive. Leaving the window, native deactivation, removal,
hiding and disabling deliver leaves too. Like pointer listeners, a hover listener
makes the element a pointer target. For paint-only feedback on controls,
`hover_style` needs no listener.

`on_wheel` receives `WheelInput` for wheel and trackpad scrolling, from the element
under the pointer up to the root (target and bubble phases, no capture phase), with
logical content motion and modifiers. Default container scrolling follows unless a
listener calls `prevent_default()`, for example to zoom a canvas on Ctrl+wheel.
`WheelInput::source` says whether the delta is precise (`WheelSource::Precise`,
trackpads) or converted mouse-wheel steps (`WheelSource::Lines`); smooth scroll
containers animate only steps. Custom hosts call `Ui::wheel` for precise deltas and
`Ui::wheel_with_source` for steps; `Ui::scroll` remains the direct default
scrolling alone.

Capture is per UI and covers one mouse gesture with primary, secondary or middle
buttons. It routes events delivered by the host; it is not an OS-wide drag session,
touch/pen API, drag-and-drop data transfer, or cross-window capture service. Dragging
within a window is the capture pattern above: capture on press, follow
`drag_delta()` on motion, and finish on release or cancellation. Native file drops
arrive through `Application::file_drop`; dragging data between elements or out of
the window is not provided.

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
`.scrollbar_size(...)` select the presentation, and
`.scrollbar_style(ScrollbarStyle::Overlay)` draws thin bars over the content
without gutters, as on macOS ([conventions](native-feel.md#platform-conventions)).
It needs bounded parent space;
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

`.scroll_behavior(ScrollBehavior::Smooth | Instant)` on a scroll element,
`scroll_area` or virtual list selects whether mouse-wheel steps and `scroll_to`,
`scroll_by` and the reveal commands animate; the default,
`ScrollBehavior::platform()`, is Smooth on macOS and Windows and Instant elsewhere.
`jump_to` always applies immediately. Trackpad deltas, scrollbar and keyboard range
input, assistive actions and focus or caret reveal stay direct and end a smooth
scroll in flight. Smooth motion needs a frame clock and honors reduce motion; see
[animation](animation.md#smooth-scrolling).

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
callbacks conservatively invalidate their owner unless they call
`cx.unchanged()`, which also reports `Dispatch::Unchanged` so the host need not
redraw; a pointer-move listener outside a drag should. Use child entities to keep
large pane descriptions outside a frequently resized parent. Stock scrollbar
motion has no application callback and changes geometry without view evaluation,
text measurement or Taffy layout unless a view explicitly reads metrics. Split
resizing changes layout and may reflow/measure text; retaining a pane description
does not remove that work.

All rows remain retained and scrolling refreshes tree geometry. There is no
virtualization or constant-cost large-list guarantee. See the
[CPU measurements](performance/interaction.md) for reproducible workloads and limits.
The later [scrolling diagnosis](performance/scrolling.md) exercises real text,
recording, queue submission and native accessibility publication; geometry-only
timings do not establish a native frame budget.
[Focus groups and controlled tabs](focus-and-tabs.md) now supply remembered
placement-local focus, keyboard cycling, selection/close proposals and retained
or selected-only panels. The [controlled docking tree](docking.md) composes those
controls and adds pointer header drops with previews. Cross-native-window detach
remains separate work.

## Raw keyboard input and deactivation

`Element::raw_keyboard(true)` suppresses keyboard defaults while the focused
route includes that element. Put it on the root to cover the whole window, or
on a focusable view for a local control mode. With no focused element the root
route applies. Listeners still receive capture, target and bubble events, including
Tab, Escape, Enter/Space, modifiers, repeats and releases. Commands, focus traversal,
control activation, gesture Escape cancellation and native text/IME defaults do
not run. `Ui::raw_keyboard()` exposes the current policy to custom hosts; native
hosting also disables IME while that route is raw. Explicit pointer and command
operations retain their usual behavior.

A root `on_key_down_capture` listener can already call `prevent_default()` to
suppress defaults for selected keys, including standard shortcuts. It does not
stop propagation; call `stop_propagation()` separately if children should not see
the key. Raw mode applies the same prevention automatically for every key in its
route and also suppresses native IME commits. Capture listeners are a useful place
to collect held input independently of the focused child:

```rust
column()
    .raw_keyboard(self.control_mode)
    .on_key_down_capture(cx.listener(|this, input: &KeyInput, _| {
        if this.control_mode && input.event.physical_key == PhysicalKey::KeyW {
            this.forward = true;
        }
    }))
    .on_key_up_capture(cx.listener(|this, input: &KeyInput, _| {
        if input.event.physical_key == PhysicalKey::KeyW {
            this.forward = false;
        }
    }))
```

`KeyEvent::key` remains the layout-dependent logical identity. `physical_key`
identifies a position independently of the active layout: for example, a French
layout can produce logical `"z"` at `PhysicalKey::KeyW`. Common positions have typed
variants, function keys use `Function(number)`, and other identified positions use
`Code(String)` with winit KeyCode spelling. `Native(String)` preserves native
unidentified codes; `Unidentified` is available to custom hosts without physical
information. Logical Shift, Control, Alt, Super, lock keys, Insert, Pause,
PrintScreen, ContextMenu and function keys now have typed `KeyboardKey` variants
instead of `Other` strings. Existing `KeyEvent` literals must supply `physical_key`;
use `Default::default()` when the host has no physical identity.

`WindowInput` tracks physical keys and RXUI's supported primary, secondary and
middle pointer buttons. Native deactivation drains the held state, sends key-up
and `PointerEvent::CancelledUp` for held buttons, cancels pointer gestures and then
calls `Ui::set_active(false)`. Cancelled button releases reach the capture owner
without clicking controls or committing a gesture; the following cancellation
ends capture. Key releases retain the press's logical and physical identities,
have `repeat: false` and cleared modifiers. Repeats do not add held keys. Late
native releases are deduplicated, and synthetic focus-gain presses never re-arm
controls. `Ui::key` accepts releases while inactive so custom hosts can safely
release after deactivating too; inactive presses remain suppressed.

`Application::window_active_changed` runs after these releases and pointer unlock,
with a `WindowHandle` and a fresh `AppContext` for updating view state.
`WindowHandle::is_active` provides a snapshot. Clear application control mode on
loss and require an explicit action to re-enter it. A key-up follows the current
focus route; the framework cannot release state in a removed listener. Applications
collecting held input should use a stable root capture listener and clear their
input snapshot on the activation callback as well.
