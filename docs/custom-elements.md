# Custom elements

Status: implemented. Measurement and hit testing need the default `layout` feature;
preparation and painting need `rendering`.

A custom element is an application-defined leaf. It takes part in layout like a
label or image, receives pointer input through the ordinary listeners, and paints
with the Astrelis painter. Implement `CustomElement` and wrap the value with
`custom(...)`:

```rust
use rxui::{CustomElement, CustomMeasure, ElementInfo, NumericValue, UiError, custom, prelude::*};
use rxui::astrelis::{PaintSession, Rect};

#[derive(PartialEq)]
struct Meter { fraction: f32 }
impl CustomElement for Meter {
    type State = ();
    fn measure(&self, request: CustomMeasure) -> [f32; 2] {
        [request.known[0].unwrap_or(120.), 8.]
    }
    fn paint(&self, _: &(), element: &ElementInfo<'_>, paint: &mut PaintSession<'_, '_>)
        -> Result<(), UiError> {
        let b = element.content_bounds;
        paint.fill_rect(Rect::new(b.x, b.y, b.width * self.fraction, b.height), element.color)?;
        Ok(())
    }
}

custom(Meter { fraction: 0.4 })
    .key("progress")
    .fill_width()
    .accessibility_role(SemanticRole::ProgressIndicator)
    .accessibility_label("Upload")
    .accessibility_numeric_value(NumericValue { value: 0.4, min: 0., max: 1., step: None })
```

## Contract

- **Description.** The value is part of the view's owned description and is rebuilt
  on every evaluation. Reconciliation keeps the retained node while the element type
  stays the same. `PartialEq` decides whether layout must measure it again: an equal
  value keeps its measurement, an unequal one is measured on the next prepare.
  Painting always uses the latest value.
- **Measurement.** `measure` returns the content size in logical units, excluding
  padding and border. `CustomMeasure` carries the sizes layout already fixed
  (`known`), the space the parent offers (`available`, a Taffy `AvailableSpace`)
  and the inherited font size. Explicit sizes from `width`, `height` and friends
  win over the measurement. Non-finite or negative results fail preparation with
  `UiError::InvalidGeometry`. Font size and typography changes measure it again.
- **Hit testing.** `hit_test` receives a point relative to the border box. It
  narrows targeting for elements that already take pointer input (pointer
  listeners, a cursor, `focusable(true)` or `PointerEvents::Block`) and for wheel
  scrolling. The default accepts the whole box.
- **Preparation.** `prepare` runs in `UiPainter::prepare` for every visible custom
  element, outside any render pass. `CustomPrepare` lends the Astrelis `Painter`
  (text, path and image preparation), the `TextSystem` for shaping, the frame's
  render format and the raster scale. Load fonts through `UiPainter::fonts_mut`
  instead, so text measurement is invalidated.
- **State.** `State` is created with `Default` on first preparation and retained
  by the painter per element identity, like prepared text. It is replaced when the
  element at that identity changes to a type with a different state type, and
  dropped when the element is removed or `UiPainter::forget` releases the placement.
- **Painting.** `paint` records draws after the element's shadow, background and
  border, and before focus decoration. The session is already transformed to
  logical units and clipped to the element's clip, including a rounded ancestor
  clip. `ElementInfo` gives bounds, resolved paint (text color, background, radii),
  typography and interaction state. Keep ink inside the element's bounds: culling
  and opacity layers only account for the bounds.
- **Semantics.** Custom elements default to `SemanticRole::Container`. Describe them
  with the ordinary builders: `accessibility_role`, `accessibility_label`, and the
  value and state builders in the [semantics contract](semantics.md).

Custom elements are leaves and reject children. Compose them with containers to
mix them with labels and images.

## Text input clients

A focusable element can take typed text and IME composition, so an application can
build its own editor (rope-backed, multi-cursor, modal) with the same keyboard
layouts, dead keys and input methods as the built-in [text inputs](text-input.md).
Mark it with `.text_input_client(true)` and listen for committed text and
composition:

```rust
use rxui::{Bounds, custom, prelude::*};

custom(EditorSurface { /* rope snapshot, carets, scroll */ })
    .key("editor")
    .fill_width().fill_height()
    .text_input_client(true)
    .ime_cursor_area(Bounds { x: caret_x, y: caret_y, width: 1., height: line_height })
    .on_pointer_down(cx.listener(|this, event: &PointerInput, _| {
        event.focus();
        // Place the caret from event.local_position.
    }))
    .on_key_down(cx.listener(|this, event: &KeyInput, _| {
        // Editing keys, shortcuts and modal commands; prevent_default() keeps a
        // key's text out of on_text_insert and Tab from moving focus.
    }))
    .on_text_insert(cx.listener(|this, event: &TextInsertInput, _| {
        this.preedit = None; // an IME commit replaces the preedit
        this.insert_at_carets(&event.text);
    }))
    .on_ime(cx.listener(|this, event: &ImeInput, _| match &event.event {
        ImeEvent::Preedit { text, cursor } => this.preedit = Some((text.clone(), *cursor)),
        ImeEvent::Cancel(_) => this.preedit = None,
    }))
```

The application owns the document, carets, selection, preedit display and
undo; RXUI only routes input and drives the platform IME. The flag works on any
element, but a custom element painting its own text is the intended client.

- **Opt-in.** `.text_input_client(value)` also makes the element focusable unless
  `.focusable(...)` was already called, for either value, so a view can switch it
  off (for example while read-only) without losing focus. The element is a client
  while it is focused, enabled (visible, not inert, inside any open modal) and the
  placement is active; `Ui::focused_text_client` reports it. Text inputs, text
  areas and selectable labels ignore the flag. A focusable element without it gets
  no text or IME events, even if it has the listeners.
- **Routing.** `on_text_insert` and `on_ime` run from the focused client up to the
  root (target and bubble phases, no capture phase), so a wrapping container can
  handle a child's text. `stop_propagation()` ends the route. Like other listeners,
  they invalidate their owner, and the next text event reconciles the updated
  description first, so successive events without a frame see the application's
  latest state.
- **Keys first.** Every key reaches key listeners, capture then bubble, before its
  text. A key-down listener that calls `prevent_default()` suppresses the text that
  key would insert, as for `j` as a motion in a Vim-style normal mode, and the
  host's default for that key. Shortcuts with Command or Control (without Alt)
  insert no text. Editing keys (arrows, Backspace, Delete, Enter, Home/End, Page
  Up/Down) arrive only as key events; clients implement them.
- **Committed text.** `TextInsertInput::source` is `Keyboard` for typed text,
  `Ime` for an IME commit and `Paste` for the standard Paste command's host default.
  The text is the platform's output, unnormalized: it can hold several characters
  or a line break. Keyboard text and paste are dropped while the client is
  composing, as in built-in inputs; an IME commit then ends the composition.
- **Composition.** `ImeEvent::Preedit` carries the composing text and the IME's
  selection or caret as UTF-8 byte offsets into it (`None` hides the composition
  caret). Display it at the caret without changing the document. An empty preedit
  clears the display while composition continues; outside composition it is not
  delivered. Invalid offsets or control characters fail `Ui::text_input` with
  `UiError::InvalidTextValue`. A commit ends composition through
  `on_text_insert` with source `Ime` and sends no cancellation; the commit may be
  empty, which still replaces the preedit.
- **Cancellation.** `ImeEvent::Cancel(reason)` ends composition without a commit:
  `Host` (Escape, or the platform disabled the IME), `FocusChanged`, `Deactivated`
  (the window lost activation), `Pointer` (a primary press on the composing client
  or its descendants, delivered before that press's listeners run, even if they
  prevent the default), `Unavailable` (the flag was switched off, or the element
  became inert, hidden or non-focusable) and `Removed`. A removed client's
  cancellation goes to the listeners captured at its last preedit, so a surviving
  owner can clean up. Cancellations that do not come from a text or pointer event
  are delivered during the next preparation, before further input; any framework
  cancellation changes `Ui::ime_reset_revision` so the host restarts the platform
  session. These mirror the built-in inputs.
- **Candidate window.** `.ime_cursor_area(bounds)` gives the caret rectangle in
  logical units relative to the element's border box. Rebuild it in the view as
  the caret moves; `Ui::ime_cursor_area` translates it to window coordinates from
  the prepared geometry, and the native host forwards changes to the platform.
  Without it, the content box is used. Non-finite values or a negative size make
  `Ui::ime_cursor_area` fail with `UiError::InvalidGeometry`.
  A custom editor that measures its caret during `CustomElement::prepare` can use
  `.ime_cursor_area_with(move || Some(geometry.borrow().caret))` instead. The
  provider reads retained application geometry when queried; the native host
  queries again after custom preparation, before submitting that same frame.
  No follow-up animation frame is needed just to publish the measured rectangle.
  The callback must only read geometry, and may also be called before preparation.
  Returning `None` uses the content box; the last static/deferred setter wins.
- **Tab.** Tab and Shift+Tab reach key listeners before the host moves focus.
  Calling `prevent_default()` on the key-down keeps focus in the client, so it can
  indent or insert a tab. Leave a way out of the editor for keyboard users, for
  example only capturing Tab in an insert mode, as the example does.
- **Clipboard and commands.** The default Paste command inserts clipboard text
  with source `Paste`. Copy, Cut, Undo, Redo and Select All are disabled for a
  client unless it registers its own handlers with `.on_command::<C>(...)`; read
  and write the clipboard through `AppContext::read_clipboard_text` and
  `write_clipboard_text`.
- **Accessibility.** A client publishes only its ordinary semantics; describe it
  with `accessibility_role` and `accessibility_label`. Its text, caret and
  selection are not exposed to assistive technology yet.

The native host enables the platform IME while a client is focused (it does not,
as it does for password inputs, refuse composition), places the candidate window
at the reported area, and routes winit's key text, `Ime::Preedit`, `Ime::Commit`
and `Ime::Disabled` to the client. It schedules no caret blink for clients;
animate the caret with the client's own timing if needed.

### Custom hosts

A custom host feeds a client through the same entry point as the built-in inputs:

1. Route each key through `Ui::key`. Unless its `default_prevented` is set, apply
   the host's Tab traversal; then, while `Ui::focused_text_client()` is `Some`,
   send the key's produced text as `TextInputEvent::Insert` and Escape as
   `TextInputEvent::CancelComposition`. Send no other editing events.
2. Send IME preedits, commits and platform cancellation as
   `TextInputEvent::Preedit`, `Commit` and `CancelComposition`, and clipboard
   text as `Paste`.
3. Enable the platform IME while `Ui::accepts_ime()` is true, restart it when
   `Ui::focused_element()` or `Ui::ime_reset_revision()` changes, and place the
   candidate window at `Ui::ime_cursor_area` after input and after each
   preparation.

```sh
cargo run -p rxui --example custom_text_client_window --features native --locked
cargo test -p rxui text_client_tests --locked
```

The [example](../crates/rxui/examples/custom_text_client_window.rs) is a small
editor with insert and normal modes, Tab indentation and IME composition. Headless
tests cover routing to a focused client and its ancestors, key precedence, preedit
and commit, every cancellation reason, Tab capture, the candidate area and that
non-clients and built-in inputs are unaffected. A GPU test drives the native
`WindowInput` with winit IME events. Native composition with each platform's input
methods is not covered by these tests.

### Crisp coverage text

For text drawn directly in the logical window coordinate system, prepare with
`cx.text_raster_options(origin)` and draw at that same `origin`. Compute the origin
only after clamping scrolling or revealing a caret. For a virtualized line it
includes the line's top, any horizontal window offset, editor insets and current
scroll. Separate overlays need their own origins.

Retain the shaped layout and prepare new GPU text only when layout, density, or
fractional physical origin changes; whole physical-pixel translations reuse the
same prepared resource. The phase key can be computed with
`origin.map(|v| (v * cx.raster_scale).rem_euclid(1.))`. Do not round only the origin
to a quarter-pixel bin for the cache key: each glyph's fractional local position
can cross a different bin boundary. The Astrelis glyph cache bounds the actual
quarter-pixel raster variants.

The helper assumes RXUI's positive uniform draw scale. If custom painting adds
rotation, shear or nonuniform scaling, select Astrelis's ordinary filtered text
preparation or use its explicit placement contract for a suitable final transform.
