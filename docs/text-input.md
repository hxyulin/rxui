# Controlled single-line text input

Status: implemented with the default layout feature, Astrelis geometry through
`rendering`, and native keyboard/clipboard/IME wiring through `native`.

## Application API

```rust
use rxui::prelude::*;

struct Form { name: String, submitted: String }
impl View for Form {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().padding(24.).gap(12.)
            .child(text_input(self.name.clone()).key("name").fill_width()
                .accessibility_label("Name")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                    this.name = edit.value.clone();
                }))
                .on_submit(cx.listener(|this, event: &TextSubmitEvent, _| {
                    this.submitted = event.value.clone();
                })))
            .child(label(self.submitted.clone()))
    }
}
```

A value belongs to application state. An input owns selection, composition and
horizontal offset in its retained placement. Compatible keyed identity retains
that state across description rebuilds/reorders. Sharing an Entity<View> shares
its fields, while each Ui placement retains its own editing state.

TextChangeEvent contains a proposed complete value and selection. Updating the
controlled property accepts the proposal. The application can supply a normalized
value instead, or leave the property unchanged to reject it. The listener event
annotation is explicit because Rust cannot infer a generic event from field access
inside `cx.listener`; method handlers with typed event arguments work as well.

RXUI reconciles dirty descriptions before a text event and after each committed
proposal, without requiring a layout computation or presented frame. Thus successive
inputs use the application's current accepted answer. A rejected proposal restores
the previous selection. Accepted proposals keep the proposed selection. A normalized
or externally replaced value clamps offsets to valid grapheme boundaries; this
first policy does not remap selections through an arbitrary edit diff.

This synchronous reconciliation is specific to ordered controlled editing. It can
rebuild the affected component's description on every committed edit. Component
boundaries remain useful when the rest of the application is large. Observers and
deferred effects still wait for the host's normal flush after updates have ended.
Acceptance belongs in the synchronous on_change handler, rather than an eventual
observer; an asynchronously supplied value is an external replacement.

`text_input` defaults to 240×44 logical units with padding and a background, and
supports the existing size/color/font builders. `.read_only(true)` permits focus,
selection and copy while preventing edits/IME. A missing on_change behaves as
read-only. `.disabled(true)` removes the input from hit testing/focus traversal.
Application-provided values cannot contain control characters. User-inserted CRLF,
line breaks and tabs normalize to spaces; other control characters are discarded.

## Editing and text geometry

Native controls support pointer caret placement/drag selection, Shift-click,
selection replacement, grapheme backspace/delete, visual Left/Right, Shift extension,
word navigation, start/end, Select All, copy/cut/paste and Enter submission. Platform
primary shortcuts use Command on macOS and Control on Windows/Linux. Alt/Option word
navigation is used on macOS; Control is used on Windows/Linux. Key repeats work for
text/editing navigation, while submission/button activation does not repeat.

Offsets are whole-value UTF-8 bytes, validated at extended-grapheme boundaries.
TextSelection retains directional anchor/focus and TextPosition includes affinity
at bidirectional boundaries. Emoji sequences and combining accents are not split
by ordinary deletion. Astrelis's shaped TextLayout supplies pointer hit tests,
visual neighbors, caret geometry and selection rectangles. The same snapshot is
used for drawing; mixed bidi selections can produce multiple highlight rectangles.

The input clips text/selection/caret to its content box, intersected with ancestor
and caller clips. Its horizontal offset follows the active caret. Focus traversal
can reveal the input through scrolling ancestors. Selection/caret changes do not
reshape text or upload glyph resources. On the warm path, interaction queries use
retained revision/width/font metadata and the cached interaction index instead of
copying the value or walking all text bytes.

The native host schedules a 500 ms caret blink through astrelis-winit redraw
deadlines only while an active input is focused. Input resets the blink. It uses
no background task or periodic application polling for this purpose. Known surface
unavailability is still handled by the runner's deadline/availability policy.

## IME lifecycle

Preedit is transient placement state. It replaces the selection captured when
composition begins, affects displayed shaping, and emits no TextChangeEvent.
Preedit cursor endpoints are byte offsets into the preedit string; invalid offsets
produce UiError. Painting maps them to valid grapheme geometry. Missing cursor
endpoints hide the composition caret while preserving a candidate-window anchor.

An empty preedit clears the visual text but retains the original replacement
selection until commit/cancellation. This handles the native empty-preedit event
immediately preceding Commit. Commit replaces that original selection once and
passes the result through normal controlled acceptance. Keyboard insertion and
submission are ignored during active composition.

Changing focus, losing native activation, Escape, pointer repositioning, disabling
editing, removal or an external value replacement cancels composition. An external
replacement cancels only that placement's composition; other windows retain their
own selections. `ime_reset_revision()` lets a native/custom host reset its IME
session even when the focused identity did not change. Normal commits do not reset
that session. Application enables IME only for an editable focused input and updates
the native candidate area from the same shaped caret geometry as painting.

The native wiring follows [winit's KeyEvent text/repeat contract](https://docs.rs/winit/0.30.13/winit/event/struct.KeyEvent.html)
and [IME/candidate-area contract](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html).
Headless editing uses [Unicode extended grapheme segmentation](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/trait.UnicodeSegmentation.html).
Native IME event streams differ by platform/input method; synthetic event tests
and GPU tests do not establish every platform's native composition behavior.

## Custom hosting and verification

Custom hosts call Ui::text_input with TextInputEvent and a TextMeasure adapter;
geometry queries are optional in a headless adapter. Without them, movement uses
logical grapheme order and pointer caret placement falls back to the value end.
UiPainter supplies the real shaped geometry. Use pointer_with_text for shaped
caret placement, set_active for native focus changes, set_caret_visible for blink,
selected_text for clipboard operations, and ime_cursor_area/ime_reset_revision for
platform IME integration. After external changes, prepare before geometry-based
input or clipboard inspection. Layout and GPU preparation remain separate.

```sh
cargo run -p rxui --example text_input_window --features native --locked
cargo test -p rxui --all-features editing_tests --locked
cargo test -p rxui --features rendering painting::tests --locked -- --ignored
```

The standalone example demonstrates shared windows, ordinary acceptance, uppercase
normalization, digit-only rejection, read-only state and external replacement.
Headless tests cover successive edits without a frame/flush, Unicode deletion,
drag/Shift selection, identity/reorder/removal, composition-clear/commit ordering,
external shared changes and disabled/read-only behavior. The GPU test exercises
mixed bidi text and combining preedit, checks stable shaped-layout identity and
interaction storage, unchanged glyph geometry/uploads, and real draw validation.

This is an initial single-line control. Undo/redo history, multiline editing,
double/triple-click word/line selection, word deletion shortcuts, password masking,
placeholder text, and advanced editing commands remain future work. Accessible
labels and the initial AccessKit tree are implemented: assistive SetValue follows
the same controlled proposal path, while selection requests validate the published
text revision and grapheme boundaries. See [the semantics contract](semantics.md).
Per-character accessibility geometry and rich text attributes remain future work.
