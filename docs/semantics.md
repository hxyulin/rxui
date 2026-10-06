# Semantics and accessibility

Status: portable semantic snapshots/actions are implemented with `layout`.
The optional `accessibility` feature adds AccessKit translation. `native` includes
that feature and manages desktop adapters through Application. State-only builds
remain independent of layout, AccessKit, windows and graphics.

## Application API

```rust
use rxui::prelude::*;

struct Form { name: String, saves: u32 }
impl View for Form {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().padding(24.).gap(12.)
            .accessibility_role(SemanticRole::Form)
            .accessibility_label("Profile")
            .child(label("Profile").accessibility_role(SemanticRole::Heading))
            .child(text_input(self.name.clone()).key("name")
                .accessibility_label("Name")
                .accessibility_description("Name shown to other users")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                    this.name = edit.value.clone();
                })))
            .child(button("Save").key("save")
                .on_click(cx.listener(|this, _, _| this.saves += 1)))
    }
}
```

Labels, buttons and text inputs infer their roles from the control kind. A button
uses its caption as its name unless explicitly named. Text inputs require an
explicit accessible name; adjacent visual labels are not automatically associated.
`accessibility_description` supplies additional help separately from name/value.
Containers can be marked Group, Form, List or ListItem. Heading is initially
published at level one. A role override describes an element; it does not add
button handlers, editing or focus behavior to an ordinary container.

`accessibility_hidden(true)` excludes the whole subtree from assistive navigation
and assistive actions, without changing paint or ordinary pointer/keyboard input.
Display:none also excludes a subtree. Children outside a scroll viewport remain
in the semantic tree with their full bounds, allowing navigation and reveal.
Custom consumers must filter structural child IDs through the same snapshot.

## Identity and actions

`Ui::semantics()` returns borrowed SemanticNode snapshots; `semantic_node(id)`
looks up one retained element. They expose role, name, help, displayed text,
selection, disabled/read-only state, supported behavior and logical geometry.
`semantic_focus()` returns focus eligible for semantic navigation. Prepare the UI
successfully before publishing or using geometry-based actions.

Element identity belongs to a Ui placement and survives compatible keyed
reconciliation. Sharing one Entity<View> between windows shares model fields,
while selection, composition, focus, scrolling and native accessibility identity
remain independent. Removed IDs are never reassigned to new elements.

`Ui::semantic_action(runtime, action, measurer)` routes portable actions:

| Action | Behavior |
| --- | --- |
| Focus | Focus the underlying control and reveal it through scroll ancestors. |
| Activate | Dispatch the same button listener as pointer/keyboard activation. |
| SetValue | Propose a complete single-line value through the existing on_change listener. |
| SetSelection | Apply directional byte/grapheme endpoints to the expected text revision. |
| Scroll | Clamp absolute logical offsets to reachable container ranges. |
| ScrollIntoView | Reveal a retained element even when it is outside the current clip. |

Unsupported, hidden, removed, foreign-placement and stale text actions return
Ok(false). Activation, focus and editing respect the control's disabled/read-only
policy; revealing an element does not require it to be enabled. Invalid selection
endpoints are rejected. Nonfinite scroll
offsets are ignored. Dirty controlled descriptions are reconciled before dispatch,
so queued text actions cannot silently apply old offsets to a replacement value.
Application normalization and rejection work exactly as with keyboard edits.
Assistive SetValue can operate while the native window is inactive. Listener
updates retain the event's source window, including nested updates.

## AccessKit translation and custom hosts

`AccessKitTree` owns one placement's translation/publication cache and no window or
GPU. Construct a separate cache for each Ui. `update(&ui, title, scale)` requires
prepared geometry and a finite positive DPI scale. It returns a complete initial
TreeUpdate, a changed-node update, or None when semantics are unchanged. Its bounds
and scroll properties are physical window coordinates. Input values use TextRun
children when representable; the consumer derives the value from those children.
This avoids duplicating whole input values in selection-only parent updates.

A custom host should:

1. Install its platform adapter before showing the window and forward native window
   events to it before routing application input.
2. Handle the adapter's activation event through the UI event loop. Mark the cache
   active and call `reset()` to request a complete tree.
3. Flush updates and prepare CPU layout with the current logical viewport, then
   call `update`. Publish each returned update through the active platform adapter.
   This step must progress independently of acquiring a renderable surface.
4. Decode native ActionRequest with `tree.action(request)`. Bring controlled state
   current before decoding, dispatch a returned SemanticAction through Ui, then
   prepare and publish again. Forward focus to the native window as appropriate.
5. On deactivation call `deactivate()` and stop constructing snapshots. On window
   removal drop the platform adapter before releasing its native window.

AccessKitTree rejects reuse with another placement. Native structural IDs are
stable, monotonically allocated and never reused. TextRun IDs change with text
revision so stale native selections cannot address replacement text. Decoding
rejects unknown/subtree IDs and unsupported actions, and validates text/value/scroll
payloads. Native
selection indices map through cached grapheme boundaries; SetScrollOffset converts
physical offsets back to logical units. Scroll Item moves 40 logical units;
Scroll Page, or a directional scroll with no unit, uses the viewport dimension.

The integration follows the versioned
[AccessKit TreeUpdate contract](https://docs.rs/accesskit/0.25.1/accesskit/struct.TreeUpdate.html)
and [accesskit_winit adapter lifecycle](https://docs.rs/accesskit_winit/0.34.1/accesskit_winit/struct.Adapter.html).
Application uses asynchronous initial-tree requests; the platform adapter can
supply its own temporary placeholder until the event loop publishes the real tree.

Application installs one adapter per window by default, before its hidden
window_created hook. `.accessibility(false)` disables this integration when a
custom hook will own an adapter. Native activation, actions and deactivation use
an event-loop proxy. Model/task progress prepares active semantic trees without
waiting for GPU acquisition. Adapter lifetimes are bounded by native windows,
including creation failure and final host teardown.

## Text scope and performance boundary

Text values and committed directional selections are exposed. Preedit contributes
to the displayed value; selection publication/actions are suppressed while
composition is active. Text units follow extended grapheme boundaries, preserving
combining accents and emoji sequences. AccessKit character lengths are u8 byte
counts; if a single grapheme exceeds 255 bytes, the field falls back to its full
value without character-selection actions. Focus, read-only state and controlled
SetValue still work. The fallback is retained by revision and retried when text
changes.

Per-character bounds/advances, text-range geometry, rich text attributes, multiline
navigation, live regions, label relationships and additional control roles are
not implemented in this first slice. Shaped pointer/caret/selection geometry used
for rendering remains available through UiPainter, but it is not yet exported as
AccessKit character geometry. Native value/selection support does not establish
full screen-reader, magnifier or every text-range interaction behavior.

Explicit metadata is allocated only when an accessibility builder is used. Native
publication is inactive until requested. An unchanged cache update checks a small
retained revision key; hover/capture and caret blinking cause no node/text work.
Selection-only updates reuse the existing text encoding and publish the field
parent without copying the text run. A meaningful semantic change currently
examines the retained semantic tree before diffing nodes, so publication of one
changed node is not a claim of constant-time changed-tree construction. Scroll
geometry and initial trees also scale with retained content. See the
[measurement report](performance/accessibility.md) for timings and scope.

## Verification

Portable tests cover inferred roles/names, offscreen children, hidden subtrees,
focus/reveal, shared listener activation, controlled normalization, revision and
Unicode validation, removal, foreign identities, disabled/read-only state and
scroll clamping without text/layout work. AccessKit consumer tests validate full
and delta trees, reorder/removal/reactivation, separate placements, text-derived
values, preedit, empty text, oversized graphemes and stale native text-run IDs.
They also assert unchanged publication counters on hover/blink and reuse of text
runs during selection updates.

A separate macOS native probe was queried through System Events accessibility.
Its named AXTextField and AXButtons were visible. AXPress incremented the model;
AXValue passed through the controlled uppercase handler and returned NATIVE EDIT.
AXPress on its close button exited cleanly. This verifies those native actions and
adapter teardown on macOS; it is not a full VoiceOver or Windows/Linux acceptance
pass. Existing user windows were not focused or edited during the probe.

```sh
cargo test -p rxui --all-features semantic_tests --locked
cargo bench -p rxui --bench elements --features accessibility --locked
cargo run -p rxui --example text_input_window --features native --locked
```
