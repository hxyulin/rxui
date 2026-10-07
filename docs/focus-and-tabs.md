# Focus groups and controlled tabs

This stage supplies window-local focus and panel lifecycle before a docking model.
Selection, tab order, titles and document lifetime are application data. RXUI owns
retained placement state, keyboard routing and weak focus destinations.

## Focus API

```rust
use rxui::prelude::*;
struct Editor { focus: FocusHandle, name: String }
impl View for Editor {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .child(button("Restore editor focus").on_click(cx.listener(|s, _, cx| {
                s.focus.focus(cx).expect("live source placement");
            })))
            .child(column().focus_scope(FocusScope::Group).focus_handle(self.focus.clone())
                .child(text_input(self.name.clone()).accessibility_label("Name")
                    .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.name = e.value.clone())))
                .child(button("Save")))
    }
}
```

`FocusHandle::new()` is a cloneable reference bound with `.focus_handle(handle)`.
Bind once per Ui; the same handle can be used by multiple windows. Resolution
uses the listener's source mount, preserving separate focus and remembered targets
when a component/model is shared. A leaf receives focus directly; a container
restores its last eligible descendant, falling back to its first eligible child.
A focusable container itself is the fallback when it has no eligible children.

`handle.focus(cx)` and `handle.blur(cx)` queue requests for the next Ui preparation.
Blur only clears focus when it is within that handle's subtree. A hidden, inert or
disabled target does not acquire focus. Successful queueing does not guarantee an
eligible target will exist when the request is applied. Initial application/model
updates have no implicit source placement.

Inside a listener, capture `handle.placement(cx)` for later async/application
updates. The resulting `FocusPlacement` is weak and checks runtime and exact
retained identity. Removed/replaced bindings and closed windows return Disposed;
foreign runtimes return WrongRuntime. It does not retain an element or window.
Duplicate bindings in one Ui produce `UiError::DuplicateFocusHandle` during
preparation; the description can be corrected and retried.

Custom hosts can call `ui.focus(element_id)` after preparation. It returns whether
focus/reveal changed and ignores foreign, hidden, removed or disabled elements.

`.focus_scope(FocusScope::Group)` remembers descendants on navigation re-entry.
`.focus_scope(FocusScope::Cycle)` makes Tab/Shift-Tab wrap inside the innermost
containing cycle scope. A scope is not automatically focused and does not make
its container focusable. Cycle constrains keyboard traversal only: pointer,
programmatic and assistive focus can still leave it. Modal dialogs will need
explicit activation/restoration, inertness and dialog semantics in a later stage.

`.tab_stop(false)` excludes a focusable control from Tab navigation while retaining
pointer, explicit and assistive focus/activation. `.focusable(false)` disables
focus eligibility itself. Non-tab-stop targets advance relative to their current
description position rather than restarting navigation at the first control.

## Controlled tabs

```rust
use rxui::prelude::*;
struct Page { selected: Key }
impl View for Page {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        tabs().key("documents").selected(self.selected.clone())
            .tab(tab("editor", "Editor", text_input("Document name")))
            .tab(tab("output", "Output", label("Build output")))
            .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.selected = e.key.clone()))
    }
}
```

`tab(key, title, content)` describes a tab. `.disabled(true)` prevents selection and
header focus; `.closable(true)` enables close proposals when the group has an
`on_close` listener. Content accepts any IntoElement, including an Entity<View>.
The group accepts `.tab(...)` or `.tabs(iterator)`, a controlled `.selected(key)`,
`.on_select(...)`, `.on_close(...)`, `.activation(...)`, `.content_policy(...)`
and `.axis(...)`, plus root sizing/layout builders.

Omitting selected means no active panel, including an empty group. A selected key
must exist and be enabled; duplicate keys and invalid selection are diagnosed by
Ui preparation. The framework does not silently replace application selection.

`TabSelectEvent { key }` is a proposal. The listener updates selection in current
owner state. Pointer activation uses the ordinary button path. Keyboard focus
moves independently from selection, allowing the listener to ignore a proposal.

`TabCloseEvent { key, next_selection }` never removes a tab itself. The callback
owns removal, unsaved-document confirmation, cancellation and selection. The
suggested selection remains unchanged for a background tab. Closing the selected
tab suggests the following enabled tab, then the preceding enabled tab, or None.
A collection with optional selection should clear selection after its last close:

```rust,ignore
.on_close(cx.listener(|s, e: &TabCloseEvent, _| {
    s.documents.retain(|doc| doc.key != e.key);
    s.selected = e.next_selection.clone();
}))
```

Application strong entities determine document lifetime. Removing a panel releases
its mount/listeners and mount-scoped tasks, without disposing an entity still held
by the application. Closing a tab can therefore differ from closing a document.

## Navigation, focus and mounting

The header strip has one ordinary Tab stop: the selected enabled header, or the
first enabled header when there is no selection. Horizontal headers handle
Left/Right; vertical headers handle Up/Down. Home/End choose the first/last enabled
header; arrows wrap and skip disabled entries. The strip scrolls/reveals overflowing
headers. Delete proposes closing a closable focused header. Routed key listeners
can prevent these defaults, and Control/Alt/Meta combinations are left to callers.
Close buttons support pointer/assistive/explicit focus and Enter/Space activation;
they do not add a stop for every tab to ordinary Tab traversal.

`TabActivation::Automatic` proposes selection with arrows/Home/End.
`TabActivation::Manual` moves only focus; Enter/Space use ordinary host button
activation to propose selection. Embedded hosts must honor Ui::key's
`default_prevented` before their own Tab, button and editing defaults. These
keyboard choices follow the [W3C tabs pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/).

Header activation keeps header focus. Tab enters the selected panel's remembered
eligible Tab target; a panel with no descendants participating in Tab navigation
is itself a Tab stop. Explicit focus restoration can also restore a control marked
`tab_stop(false)`.
Changing selection while focus is in the old panel restores focus within the new
panel. Removing a focused header/close control chooses the selected surviving
header, then another enabled header or a focusable control outside the group.
An explicit queued focus request is applied after this default restoration.

`TabContentPolicy::KeepMounted` is the default. Inactive panels use display:none,
retaining keyed mounts, selection, scroll offsets and remembered focus. They have
no visible geometry, input or semantics. They can still evaluate changed model
data, and retain resources/subscriptions/tasks; this is not suspended execution.
Scroll handles have no current visible binding while hidden. Offsets survive and
are clamped against fresh content dimensions when shown again.

`TabContentPolicy::MountSelected` describes only the selected panel. Switching
releases the old placement/widget state and cancels mount-scoped work. Returning
creates fresh placement identity while strong application entities retain data.

Both policies use stable keys. Reordering headers/panels preserves compatible
placement identity. Moving a panel to a different structural parent/window is not
an identity transfer; docking reparenting and native detach remain later work.
Shared entities share selection only when selection is stored in the shared entity.
To keep selection per window, use separate workspace/view entities referencing
shared document entities. Focus, caret and scroll remain placement-local.

## Semantics, example and checks

Portable semantics add TabList, Tab and TabPanel roles, explicit selected state,
orientation and live header/panel associations. AccessKit publishes these roles,
selection and references. Hidden panels are excluded; associations point only to
live semantic nodes. A role override alone does not install tab behavior.

Run the standalone [tabs_window example](../crates/rxui/examples/tabs_window.rs):

```sh
cargo run -p rxui --example tabs_window --features native --locked
```

It demonstrates editable document entities, close/new requests, retained or
selected-only mounts, focus restoration, cycling with Escape release and shared
windows. It has no support module or test mode. Native checks use a temporary copy
with normal window attributes and computer use; no always-on-top setting.

The macOS computer-use check exercised accessibility selection, note editing,
switching away and back under both mounting policies, new documents, closing a
background and selected document, and recreating a document after closing the last
tab. The native window's panel viewport and scrollbar were inspected visually,
then the temporary window was closed. Keyboard traversal and retained scroll
offsets are covered by the automated placement tests. The real-font GPU test also
checks intrinsic grid measurement and finite entity-backed scrolling viewports.

Tests cover source-window focus resolution, scope cycling/restoration, weak
placement/runtime errors, duplicate bindings, retained editing/scroll, mount
policy, keyboard activation/close/disabled behavior, prevention/orientation,
validation/retry, key reordering, empty groups and AccessKit associations. These
are functional contracts, not performance results or a full VoiceOver usability
certification. A later optimization pass can address scans/allocation and dormant
component scheduling separately.

The [controlled docking tree](docking.md) now composes these groups with splits and
checked programmatic edits. Drag previews/reordering gestures and native
cross-window detach follow that model and its lifecycle contracts.
