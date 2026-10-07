# Viewport overlays and typed commands

Open state and command availability belong to the application. RXUI supplies
placement, input defaults, focus, painting and portable/native semantics. The
standalone [desktop_window example](../crates/rxui/examples/desktop_window.rs)
combines anchored menus, dock-header context requests, confirmation dialogs,
window shortcuts and an application fallback:

```sh
cargo run -p rxui --example desktop_window --features native --locked
```

## Typed actions and scoped routing

Implement the `Command` marker for each action type. `cx.command(value, callback)`
creates an owned `CommandAction<C>` bound weakly to the current component/mount.
Payloads do not need Clone or Default. A callback receives current mutable owner
state and its ordinary context, using the same access/disposal rules as listeners.

```rust
use rxui::prelude::*;
struct Increment;
impl Command for Increment {}
struct Counter { value: usize }
impl View for Counter {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let increment = cx.command(Increment, |s, _, _| s.value += 1)
            .label("Increment")
            .enabled(self.value < 100)
            .shortcut(Shortcut::primary("k"));
        column()
            .on_command(increment.clone())
            .child(increment.button())
            .child(menu().child(menu_item(&increment)))
    }
}
```

`Element::on_command(action)` installs the action in that element's scope.
Shortcut and `Ui::dispatch_command::<C>` resolution walk from the focused element
through its logical ancestors, ending with the window root, then application
registrations. Each matching action uses its bound payload. A missing/disposed
binding falls through; the nearest disabled binding shadows outer matches.
Duplicate command types or equivalent chords within one scope produce
`UiError::AmbiguousCommand`. Separate nested scopes can deliberately override them.

`action.button()` and `menu_item(&action)` explicitly target that action instance;
they do not redirect according to incidental focus. Both share its caption,
availability and weak callback. `action.invoke(cx)` provides explicit invocation
inside an update scope and returns Handled, Disabled or Unhandled. It is useful
for completions/application actions that already have an explicit target.

Description properties are snapshots. Reevaluate a view to change its caption,
enabled state or chords. Availability is not a substitute for application checks
inside callbacks, such as checking that a document still exists.

Routed key listeners run first and may prevent defaults. Commands run before menu,
tab, range and host editing/activation defaults. Only key-down invokes commands.
Chords match modifiers exactly; character keys compare ASCII case-insensitively.
`Shortcut::new(KeyboardKey, Modifiers)` supports named keys, and
`Shortcut::primary("k").shift()`/`.alt()` build common character chords. Primary
means Command on macOS and Control elsewhere. These are logical keys, not physical
scancodes or IME text insertion. Multi-key sequences and runtime rebinding are
separate features. Native repeat is suppressed by default; `.repeat(true)` enables
it. Matching repeat events are consumed without an invocation when repeat is off.

Inside a modal, background and application bindings are suspended by default.
`.allow_in_modal(true)` explicitly enables an outer/global action such as Help or
Quit. Disabled matching actions still shadow outer bindings.

## Application registration and source windows

`AppContext::command(value, callback)` creates an application action whose callback
receives `&C` and `&mut AppContext`. It checks runtime identity. Use weak entity
captures when the action should not retain documents or views.

`cx.register_command(&action)` returns a cloneable `CommandRegistration`. Keep it
alive; the runtime holds only a weak registration and dropping its last clone
unregisters it. Later registrations take precedence over earlier ones. Scoped
window actions precede all application registrations. `registration.replace`
refreshes its snapshot inside a context belonging to the registration's runtime.
Buttons referencing an older action retain that description until reevaluated.

UI dispatch supplies the source window's root mount for application callbacks;
component callbacks use their own bound source mount. Shared entities therefore
retain independent window focus/scroll destinations. Headless hosts can dispatch
through a prepared `Ui`; none of this requires a native window or GPU.

## Controlled popovers and dialogs

Bind `AnchorHandle::new()` with `.anchor_handle(handle.clone())` on any element.
`popover(handle, content)` follows its window-local border bounds.
`popover([x, y], content)` uses a logical point, suitable for context menus.
`modal(content)` centers a dialog in the viewport. Mount overlays conditionally
while open and give them stable keys:

```rust
use rxui::prelude::*;
struct Page { anchor: AnchorHandle, open: bool }
impl View for Page {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut root = column().child(button("Actions")
            .anchor_handle(self.anchor.clone())
            .on_click(cx.listener(|s, _, _| s.open = true)));
        if self.open {
            root = root.child(popover(self.anchor.clone(), label("Actions"))
                .key("actions")
                .width(240.)
                .placement(PopoverPlacement::BottomStart)
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.open = false)));
        }
        root
    }
}
```

Popovers prefer BottomStart, with a 4-unit gap and 8-unit viewport margin.
BottomEnd, TopStart, TopEnd, RightStart and LeftStart are supported. Placement flips
along its main axis if the opposite side fits, then clamps to the viewport.
Requested surface width/height are bounded against viewport space; oversized
content scrolls vertically within the clipped surface. Application content can
provide explicit scroll areas/bars for both axes. Spacing/point coordinates must
be finite; gaps/margins must be nonnegative. Duplicate anchor bindings within one
Ui are diagnosed. `Ui::anchor_bounds` exposes current visible anchor geometry to
custom hosts; it neither subscribes nor resolves another window's placement.

An element anchor that is removed, hidden, inert or completely clipped suppresses
its overlay and proposes AnchorUnavailable once until the anchor recovers.
Scrolling updates anchor placement without rerunning Taffy layout or evaluating
the owner. Such unavailability contributes to `needs_prepare` so the application
receives the proposal at preparation. Point anchors are independent of elements.
An overlay cannot anchor to a descendant of itself.

`DismissEvent` proposes Escape, OutsidePointer or AnchorUnavailable. The listener
owns open state and may reject the proposal. Escape/outside presses still stay
within the overlay when no listener is supplied. Outside presses are consumed,
including the associated release, rather than clicking a control behind the
closing overlay. Routed prevention can override these defaults. Captured gestures
receive Escape cancellation before a subsequent Escape dismisses the overlay.

## Focus, menus and modal boundaries

Opening acquires the first eligible surface descendant, with the surface itself
as a fallback. Popovers can opt out with `.autofocus(false)`; modal dialogs always
acquire focus. Tab traversal cycles within the surface. Closing restores a live,
eligible previous focus destination, falling back to a focusable element anchor
when activation supplied no focused opener. Explicit queued focus requests take precedence.
Nested dialogs restore their parent dialog, and a menu-to-dialog handoff retains
the original opener for restoration after the dialog closes. Removed or disabled
openers do not receive focus.

The innermost visible modal blocks background pointer, wheel, keyboard focus,
programmatic focus and assistive actions. Background painting continues beneath
the scrim. Background nodes are excluded from actionable semantic output while
structural ancestors remain available to connect the tree. Nonmodal popovers
consume outside pointer presses but do not disable background programmatic or
assistive focus. State remains application-owned: sharing a view entity also
shares its open state. Use separate per-window view entities referring to shared
document entities when menus/dialogs should be independent.

`menu()` adds Menu semantics and arrow navigation. Up/Down wrap through enabled
MenuItem descendants; Home/End jump to the endpoints. Enter/Space use ordinary
button activation. `menu_item(&command)` adds a MenuItem role and quiet button
styling. Command callbacks explicitly close the controlled menu. Submenus,
checked/radio items, typeahead, hover-open delays, native OS menu bars and noninteractive
tooltips remain separate extensions; this is an in-window popup menu foundation.

`Dock::on_context_menu` supplies a `DockContextEvent` with group, panel and logical
position. Secondary presses on actual dock headers, Shift+F10 and ContextMenu keys
request a menu without changing selection or beginning a drag. Close controls and
nested ordinary tabs retain their own behavior. Applications provide the menu and
validate any deferred/stale panel operation through checked `DockTree` edits.

## Rendering and accessibility

An overlay retains its logical parent, component mount, inherited theme and event
route. Its independent Taffy root does not consume parent layout space. Its viewport
paint layer escapes ancestor clipping and group opacity; local/content opacity
still uses ordinary isolated composition. Normal popovers paint above ordinary
content regardless of sibling z. Modal scopes and their child portals paint above
unrelated popovers. Geometry/hit testing use the same order and viewport clipping.
No extra framebuffer/pass is needed for ordinary opaque overlays.

`ElementInfo::viewport_overlay` marks the boundary for custom painters. Keep
logical ownership distinct from visual parentage when implementing composition.
`SemanticNode::viewport_overlay` marks native adapter boundaries: AccessKit attaches
these to the window root and publishes the surface's positioned transform. Dialog,
Menu and MenuItem roles and modal state are published. Custom hosts still honor
`Ui::key`'s default_prevented before text/control defaults and apply queued focus
commands during preparation.

Headless tests cover routing/availability, registration disposal/runtime checks,
modifier/repeat/prevention, placement/focus/cancellation, nested dialogs, anchor
scrolling and context requests. An AccessKit consumer validates lifted coordinates
and modal trees. A native GPU readback checks clipping/z/opacity escape, backdrop
pixels, 4x MSAA, 2x DPI and caller viewport/scissor preservation. Native macOS
computer-use checks exercise menus, keyboard activation, context-targeted closing,
window/application shortcuts and modal input/focus. Temporary probe windows use
normal attributes with background creation and are closed afterward. These are
functional checks, not frame-pacing measurements or full screen-reader certification.
