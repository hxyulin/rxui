# Controlled text input and text areas

Status: implemented with the default layout feature, rxui-draw geometry through
`rendering`, and native keyboard/clipboard/IME wiring through `native`. This covers
single-line inputs, multiline text areas, placeholders, password masking and
selectable labels. [Fonts](fonts.md) covers font registration and fallback, and
[rich text](rich-text.md) covers styled spans within one paragraph and the design
for links. Applications that own their editor, such as a code editor with its own
document model, make a custom element a
[text input client](custom-elements.md#text-input-clients) instead: it receives the
same committed text and IME composition through listeners.

## Application API

```rust
use rxui::prelude::*;

struct Form { name: String, password: String, notes: String, submitted: String }
impl View for Form {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().padding(24.).gap(12.)
            .child(text_input(self.name.clone()).key("name").fill_width()
                .accessibility_label("Name")
                .placeholder("Your name")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                    this.name = edit.value.clone();
                }))
                .on_submit(cx.listener(|this, event: &TextSubmitEvent, _| {
                    this.submitted = event.value.clone();
                })))
            .child(text_input(self.password.clone()).key("password").fill_width()
                .password(true)
                .accessibility_label("Password")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                    this.password = edit.value.clone();
                })))
            .child(column().scroll_y().height(240.)
                .child(text_area(self.notes.clone()).key("notes").fill_width()
                    .accessibility_label("Notes")
                    .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                        this.notes = edit.value.clone();
                    }))))
            .child(label(self.submitted.clone()).selectable(true))
    }
}
```

A value belongs to application state. An input owns selection, composition, bounded history and
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

`text_input` takes the theme's input width and height (240×36 in the default theme)
with padding and a background, and supports the existing size/color/font builders.
`.read_only(true)` permits focus, selection and copy while preventing edits/IME. A
missing on_change behaves as read-only. `.disabled(true)` removes the input from hit
testing/focus traversal. Single-line values cannot contain control characters.
User-inserted CRLF, line breaks and tabs normalize to spaces; other control
characters are discarded.

## Placeholders

`.placeholder(text)` paints a hint in the theme's `TextMuted` color while the value
is empty and no IME composition is active; the caret stays at the start. The hint is
shaped and prepared separately from the value, only while it is shown, and does not
affect the input's size. `ElementInfo::placeholder` and
`TextInputInfo::placeholder_color` expose it to custom painters. Assistive
technology receives it as the AccessKit placeholder, independent of the accessible
name and the empty value. A placeholder is not a label: still name the input with
`.accessibility_label`.

## Password inputs

`.password(true)` displays one bullet (U+2022) per grapheme. The value stays the
application's plain text, and edits, undo and controlled acceptance work as usual.
Masking also changes behavior:

- `Ui::selected_text` returns None, so Copy and Cut are unavailable, including in
  the native Edit menu. Paste works.
- Word movement and word deletion act on the whole value, and double-click selects
  everything, so neither reveals word boundaries.
- IME composition is refused (`Ui::accepts_ime` is false), and the native host keeps
  the platform IME disabled. Committed text from a custom host is still inserted.
- Assistive technology sees a PasswordInput whose value and selection address the
  mask, not the secret.

`TextInputInfo::selection` stays in value offsets; `display_selection` addresses
the mask for painting. Password masking requires a single-line input.

## Text areas

`text_area(value)` is a multiline input with the same controlled, history and IME
contract. Values may contain `\n` and tabs but no other control characters; typed
or pasted CRLF/CR become `\n`. Text wraps at the content width, and the area grows
with its content from a minimum of four lines (override with `.min_height`). For
long documents, place it in a scrolling container: caret movement, edits, pointer
drags and IME preedit reveal the caret through scrolling ancestors once layout is
current, so the caret stays visible as the document grows. An explicit height
clips overflowing text without scrolling it. Wheel scrolling elsewhere is not
undone later; only caret changes reveal.

| Key | Text area | Single-line input |
| --- | --- | --- |
| Enter | Line break, or submit per `SubmitShortcut` | Submit |
| Up/Down | Previous/next visual line, keeping the goal column; past the first/last line, the start/end | Start/End |
| Page Up/Page Down | Move by the visible height less one line | Start/End |
| Home/End | Visual line start/end (Windows/Linux); value start/end (macOS) | Start/End |
| Command+Left/Right (macOS) | Visual line start/end | Start/End |
| Command+Up/Down (macOS), Control+Home/End | Value start/end | Start/End |
| Tab | Moves focus; type tabs by pasting | Moves focus |

`.submit_shortcut(SubmitShortcut::PrimaryEnter)` (the default) submits on
Command+Enter on macOS and Control+Enter elsewhere; `SubmitShortcut::Enter` submits
on Enter and inserts a line break on Shift+Enter, as in chat composers;
`SubmitShortcut::Never` never submits from the keyboard. Held Enter keeps inserting
line breaks but never submits more than once.

The goal column is the caret's X when the first of a run of vertical moves began;
any other selection change resets it. Line start/end use the lower and higher
offsets at the line's two edges, so right-to-left lines start at their logical
start, and a line end at a soft wrap keeps upstream affinity on that visual line.
Triple-click selects the paragraph including its line break. Word movement crosses
line breaks. Undo groups, coalescing and the history budget are the same as for
single-line inputs; line breaks start a new group like other whitespace.

IME preedit spans lines like committed text: the candidate window follows the
preedit caret, which may be on a later visual line than the composition start.

Assistive technology sees a MultilineTextInput with one text run per paragraph,
each ending with its line break. Runs are per paragraph rather than per visual
line and carry no character geometry yet.

### Large documents

Long wrapped text (16 or more paragraphs, in text areas and labels alike) is laid
out, prepared and drawn in chunks of about eight paragraphs, each with its own
rxui-draw buffer. An edit re-chunks and re-lays out only the paragraphs it touched,
and painting records only chunks inside the clip. Caret movement does no text work.
Measured per keystroke with a frame, typing takes 0.03–0.1 ms at 100–1,000 lines and
0.14–0.18 ms at 5,000 lines of 61 bytes (0.24–0.28 ms with accessibility active);
caret movement takes under 1 µs. See the [text area measurements](performance/text.md).

Limits:

- Every edit copies and compares the whole value a few times (proposal, accepted
  copy, description, reconciliation, history); that part grows linearly and is most
  of the remaining cost at several hundred kilobytes.
- A single paragraph is the unit of layout. Very long lines without breaks (for
  example minified text) re-lay out as a whole.
- The first layout and any width or font change lay out every chunk.
- With accessibility active, each edit republishes the list of paragraph runs; a
  5,000-line selection change costs about 45 µs.
- There is no find/replace, line numbering, code folding or syntax highlighting;
  these are application or future-widget concerns.

## Selectable labels

`label(text).selectable(true)` lets pointer drags, double-click (word) and
triple-click (paragraph) select static text, and Select All and Copy work while it
is focused. The label takes pointer focus but stays out of Tab traversal; add
`.tab_stop(true)` after `.selectable(true)` to include it. Shift+arrow keys extend
its selection. It shows the text cursor on hover, no caret, and accepts no edits.
Its selection collapses when focus moves elsewhere. The label keeps its alignment
and wrapping. Assistive technology receives the selection through a text run.

## Editing and text geometry

Native controls support pointer caret placement/drag selection, Shift-click,
selection replacement, grapheme backspace/delete, visual Left/Right, Shift extension,
word navigation/deletion, start/end and line deletion, Select All, copy/cut/paste,
undo/redo and Enter submission. Double-click selects a Unicode word-boundary segment
(including punctuation or whitespace); dragging extends by segments. Triple-click
selects the single line, or the paragraph in a text area. Shift-click remains
ordinary caret extension. Platform primary shortcuts use Command on macOS and
Control on Windows/Linux. Alt/Option word navigation is used on macOS; Control is
used on Windows/Linux. Key repeats work for text/editing navigation and line
breaks, while submission/button activation does not repeat.

Offsets are whole-value UTF-8 bytes, validated at extended-grapheme boundaries.
TextSelection retains directional anchor/focus and TextPosition includes affinity
at bidirectional boundaries and soft wraps. Emoji sequences and combining accents
are not split by ordinary deletion. Grapheme and word boundaries are found by local
segmentation around the offset (words within its paragraph), so their cost does not
grow with the length of the value. rxui-draw's shaped TextLayout supplies pointer hit
tests, visual neighbors, caret geometry and selection rectangles; vertical movement
hit-tests the goal X one line or page away. The same snapshot is used for drawing;
mixed bidi selections can produce multiple highlight rectangles.

The input clips text/selection/caret to its content box, intersected with ancestor
and caller clips. A single-line input's horizontal offset follows the active caret.
Focus traversal can reveal the input through scrolling ancestors. Selection/caret
changes do not reshape text or upload glyph resources. On the warm path, interaction
queries use retained revision/width/font metadata and the cached interaction index
instead of copying the value or walking all text bytes.

The native host schedules a 500 ms caret blink through rxui-host redraw
deadlines only while an active input is focused. Input resets the blink. It uses
no background task or periodic application polling for this purpose. Known surface
unavailability is still handled by the runner's deadline/availability policy.

## Undo and redo

`TextInputEvent::Undo` and `Redo` go through the same synchronous `on_change`
proposal path. They restore value and directional selection when accepted;
rejection preserves both stacks and the prior selection. Ordinary normalized edits
record the application's actual answer. If an application normalizes a replay to a
different value, that answer is accepted and the incompatible history is cleared.
Preedit creates no history; an IME commit is one separate transaction. Replay is
unavailable during composition, for disabled/read-only inputs, or without on_change.

History belongs to the input placement and survives compatible keyed rebuilds.
An external controlled replacement clears it, including edits made by another
window sharing the same value. Applications with a shared document history can
override `standard_commands::Undo` / `Redo` in the existing command scopes.
This field history is not a document or cross-window history service.

Adjacent single-grapheme, non-whitespace `Insert` events coalesce. Consecutive
ordinary backspace/delete events also coalesce. Navigation, pointer selection,
focus/activation changes, composition and a different operation break the group.
`Paste` and `Commit` are separate transactions; custom hosts should distinguish
clipboard insertion from keyboard Insert. Grouping is deterministic, with no
clock-based timeout. Each step stores only the changed byte range and the text
that restores it, found from the common prefix and suffix of the old and new
values, so a large document's history costs its edits rather than copies of the
document. History retains at most 128 steps and 1 MiB of restoring UTF-8 across
both stacks. Old/oversized steps are discarded without rejecting edits; the current
application value is not restricted by that history budget.

Native shortcuts use Command-Z / Command-Shift-Z on macOS and Control-Z /
Control-Y (also Control-Shift-Z) on Windows/Linux. Option-backspace/delete uses
word deletion on macOS; Control-backspace/delete does so elsewhere. Command-
backspace/delete deletes to the start/end on macOS. Undo/Redo native menu entries
publish current availability and support scoped application overrides.

`Ui::text_input_info(id)` exposes selection/composition and `can_undo`/`can_redo`
without prepared geometry. It reflects the last reconciled controlled state.

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
that session. Application enables IME only for an editable, unmasked focused input
(`Ui::accepts_ime`) and updates the native candidate area from the same shaped
caret geometry as painting. A focused
[text input client](custom-elements.md#text-input-clients) also enables IME; its
composition follows the same cancellation rules and is delivered to its listeners,
with the candidate area it reports.

The native wiring follows [winit's KeyEvent text/repeat contract](https://docs.rs/winit/0.30.13/winit/event/struct.KeyEvent.html)
and [IME/candidate-area contract](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html).
Headless editing uses [Unicode extended grapheme segmentation](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/trait.UnicodeSegmentation.html).
Native IME event streams differ by platform/input method; synthetic event tests
and GPU tests do not establish every platform's native composition behavior.

## Custom hosting and verification

Custom hosts call Ui::text_input with TextInputEvent and a TextMeasure adapter;
geometry queries are optional in a headless adapter. Without them, Left/Right use
logical grapheme order, Up/Down move between paragraphs by grapheme column (ten
paragraphs per page), line start/end use the paragraph, and pointer caret placement
falls back to the value end. UiPainter supplies the real shaped geometry. Send
`TextInputEvent::Enter { modifiers, repeat }` for the Enter key so text areas can
apply their submit shortcut; `Submit` always submits and `Insert("\n")` inserts a
line break in a text area. Use pointer_with_text for shaped caret placement, or
pointer_with_text_clicks with a host-supplied click count for word/line selection.
The native host uses a 500 ms / four-logical-pixel threshold and requires the same
target; dragging or deactivation resets the click sequence. Use set_active for
native focus changes, set_caret_visible for blink, selected_text for clipboard
operations, and accepts_ime, ime_cursor_area and ime_reset_revision for platform IME
integration. After external changes, prepare before geometry-based input or
clipboard inspection. Layout and GPU preparation remain separate. While
`Ui::focused_text_client` is `Some`, `text_input` routes `Insert`, `Paste`, `Commit`,
`Preedit` and `CancelComposition` to that client's listeners and ignores the other
events; see [custom hosts](custom-elements.md#custom-hosts).

```sh
cargo run -p rxui --example text_input_window --features native --locked
cargo run -p rxui --example text_area_window --features native --locked
cargo test -p rxui --all-features editing_tests --locked
cargo test -p rxui --features rendering painting::tests --locked -- --ignored
```

The single-line example demonstrates shared windows, ordinary acceptance, uppercase
normalization, digit-only rejection, read-only state, external replacement,
placeholders, a password field and selectable text. The text area example shows a
long scrolling document, a composer that submits on Enter, selectable messages and
runtime font registration.

Headless tests cover successive edits without a frame/flush, Unicode deletion,
drag/Shift selection, identity/reorder/removal, composition-clear/commit ordering,
external shared changes, disabled/read-only behavior, line breaks and submit
shortcuts, vertical/line/page movement with and without geometry, caret reveal in a
scrolling ancestor, paragraph clicks, deep history in a 200 KB document, password
masking and clipboard availability, placeholders and selectable labels. GPU tests
exercise mixed bidi text and combining preedit with stable shaped-layout identity
and unchanged glyph uploads, wrapped vertical movement and line ends at soft
wraps, painted placeholders, and chunked long-text geometry against a single
buffer through edits.

Accessible labels and the AccessKit tree are implemented: assistive SetValue follows
the same controlled proposal path, while selection requests validate the published
text revision and grapheme boundaries. See [the semantics contract](semantics.md).
Per-character accessibility geometry, find/replace and other document-editing
commands remain future work.

## Explicit committed editing transfer

Structural moves can remount built-in fields. Applications can opt into transferring
committed selection (direction and affinity) and the existing bounded undo/redo
stacks. This is an app-owned editing snapshot, not retained mount or document history.

Bind a session-specific `TextEditingHandle` and `TextEditingIdentity::new(document_key,
version)` to the field with `.text_editing_handle(handle.clone())` and
`.text_editing_identity(identity)`. The document key is application identity, separate
from sibling-local element keys. Advance the application version on accepted or
external value changes. Exact controlled UTF-8 bytes, document identity/version,
single/multiline and password mode must match at restoration. Node `text_revision`
is placement-local and is never used as portable identity. Reusing a placement for
a different document resets selection and history, even for equal bytes. A version
increment alone preserves the valid editing session.

An ordinary application handler can request capture before changing topology:

```rust,ignore
// No Ui access is needed inside cx.listener.
let ticket = self.editing_handle.capture()?;
self.saved_editing = Some(ticket);
// Publish the accepted dock move here. In the replacement field description:
text_area(self.value.clone())
    .text_editing_identity(TextEditingIdentity::new(self.document_key.clone(), self.version))
    .text_editing_handle(self.editing_handle.clone())
    .restore_text_editing(self.saved_editing.clone().unwrap());
```

The queue captures the source's last reconciled committed state before its next
description refresh tears it down. Capture is nondestructive and does not cancel
composition or pointer capture. Normal source removal/deactivation keeps its usual
cancellation behavior. Capturing from `on_change` while its proposal is unsettled
returns `Busy` in the ticket; capture from a later command instead. Dirty external
model changes are not silently accepted into the snapshot.

The replacement applies a ready ticket before layout. Destination-first preparation
in another `Ui` keeps the request pending without continuously dirtying frames;
source readiness makes the destination's `needs_prepare` true. `Application` checks
all windows for that work during its normal command flush. Custom hosts must check
`needs_prepare` for their Uis after source preparation and request destination work.
If the source Ui is dropped before processing, the ticket resolves to `Disposed`.

`ticket.snapshot()` reports pending/success/failure; `ticket.restore_results()` makes
attempted destination results available to app handlers without a Ui or ElementId.
Failed validation leaves the destination unchanged and the snapshot available.
After repairing the destination contract, call `ticket.retry_restore()` and prepare
again. Failures do not repeatedly dirty frames. A failed capture needs a fresh
capture request. Remove the restore property when the transfer has been handled;
a retained ticket will otherwise be attempted again on a later remount.

Custom hosts can use `Ui::capture_text_editing(id)` and
`Ui::restore_text_editing(id, &snapshot)` directly, or make a ready ticket using
`TextEditingCapture::from_snapshot(snapshot)`. Reconcile external changes first.
Weak handles never retain nodes/windows and old ElementIds remain stale. Duplicate
live bindings return `Ambiguous`; use independent handles for independent sessions
(including simultaneous views of the same document), or choose an explicit Ui/id.

Preedit, pending/unaccepted edits, pointer capture/drag, focus, scroll offsets,
pixel geometry, vertical goal columns, caret animation, hooks and tasks do not
transfer. Capture excludes preedit while retaining the original committed selection;
restore refuses to overwrite destination composition, an unsettled proposal or
an active destination text drag/capture.
Geometry/theme/layout rebuild in the destination. Restored undo/redo still propose
through its current controlled handler: rejection preserves the chain, normalization
of replay clears it. A transfer ends typing coalescence. Focus requires a separate
explicit `FocusHandle::focus`/host request, and never implies native activation.
Snapshots contain exact text and history, including password values; keep them in
trusted app memory. They have no serialization or cross-process persistence API.

Run `mbx run -p rxui --example text_editing_transfer_window --features native --locked`
for an ordinary app-handler example that splits/merges a dock editor and separately
opts into focus. Automated tests exercise remounts and separate Uis in both orders.

Restore readiness examines only fields carrying restore tickets. The index is
refreshed with descriptions after reconciliation; pending captures and explicit
retries remain observable without scanning ordinary inputs on every
`Ui::needs_prepare` call. Binding publication still scans editors during refresh.

The deterministic CPU benchmark `mbx bench -p rxui --bench editing_idle --locked`
measures `needs_prepare` and a complete idle `prepare` separately on 16/256/1,000
ordinary inputs, without editing handles, identities or tickets. On Apple M5 Max,
macOS 27.0.1, Rust 1.99.1, optimized default-feature builds, the first run's batch
medians for readiness were 0.035/0.342/1.493 µs before and
0.017/0.015/0.016 µs after indexing. Idle preparation was
0.356/3.718/16.597 µs before and 0.344/3.717/14.996 µs after; it retains other
per-editor work and still scales with field count. These are local CPU timings,
not shaping, GPU or native event-loop measurements. Interactive load and test
compilation affected the repeat pair; the readiness scaling change persisted,
but full-prepare speedups should not be generalized. Each case warms 10,000
operations and records 20 batches of 10,000; setup is excluded and retained stats
are checked unchanged. See [exact metadata](performance/text/editing-idle/metadata.json),
[before CSV](performance/text/editing-idle/before.csv),
[after CSV](performance/text/editing-idle/after.csv), and the
[before](performance/text/editing-idle/before-repeat.csv)/[after](performance/text/editing-idle/after-repeat.csv)
repeat pair.
