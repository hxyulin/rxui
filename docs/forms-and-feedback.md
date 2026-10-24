# Forms and feedback

`field` labels a control and shows its help or error text. `number_field` edits
a number with a stepper, and `disclosure` shows or hides a group of settings.
`callout` is an inline status banner, and `toasts` stack short-lived
notifications in a window corner. Like the other
[standard controls](controls.md), each is controlled: the application owns the
values and accepts typed proposals.

## Fields

```rust
field("Tab width", number_field(cx, self.tab).range(1., 16.)
        .on_change(cx.listener(|s, e: &NumberEvent, _| s.tab = e.value)))
    .help("Spaces per indent level.")
field("Font family", text_input(&self.font)).error(self.font_error.as_deref())
```

A field stacks a medium-weight label, the control and a caption line in a
column with a 4-unit gap. `.inline(label_width)` puts the label to the left at
that width and indents the caption under the control, for settings pages.

The field names its control. The control is the first text input or button in
the element passed to `field`, depth first, outside popovers. That is the input
of a `text_input`, `number_field` or `combobox`, and the trigger of a `select`.
The field points the control's `labelled_by` at the label and publishes the help
or error text as the control's accessible description.

`.help(text)` shows muted caption text. `.error(Some(text))` replaces it with
the text in the `Danger` color beside a 12-unit `CIRCLE_ALERT` icon, and marks
the control invalid with `accessibility_invalid(true)`. `.error(None)` clears it.

An invalid control publishes AccessKit's invalid state. Its default paint draws
the border in `Danger` in the rest, hover and pressed states, so text inputs,
selects, comboboxes and number fields all show the error without their own
support. A disabled control keeps the disabled border, and a focused input keeps
the focus border. Any element can call `accessibility_invalid` directly.

## Number fields

```rust
number_field(cx, self.size).range(6., 72.).step(0.5)
    .accessibility_label("Font size")
    .on_change(cx.listener(|s, e: &NumberEvent, _| s.size = e.value))
```

A number field is a text input showing the value with `.precision(decimals)`
decimal places. The precision defaults to the decimal places of `.step(step)`:
0 for the default step of 1, and 2 for 0.25. The range defaults to no limit.
Every proposed value is clamped to `.range(min, max)` and rounded to the
precision, and `NumberEvent { value, phase }` reports it.

| Input | Effect | Phase |
| --- | --- | --- |
| Typing | Edits a placement-local draft. Nothing is proposed. | |
| Enter, or focus leaving | Commits the draft. Text that parses as a finite number is proposed; other text reverts to the value. | `Commit` |
| Escape | Reverts the draft. | |
| Up, Down | One step from the draft when it parses, else from the value. | `Keyboard` |
| PageUp, PageDown | Ten steps. | `Keyboard` |
| Stepper press | One step, then a step every 60 ms after it is held for 400 ms. | `Step` |
| Assistive SetValue, Increment, Decrement | Proposes the value. | `Accessibility` |

Without a draft, Enter and Escape are left unhandled, so a form or dialog
receives them. A value equal to the current one is not proposed. `NumberPhase`
is its own type: the slider's drag phases do not apply to a text field.

The stepper sits inside the input's border at its end: a 20-unit column of two
quiet chevron buttons with 10-unit icons, behind a 1-unit `Divider` line. A
press steps without moving focus, and the buttons are not Tab stops. Holding
repeats through view deadlines on the frame clock. The repeat proposal runs at
the runtime's next flush, since a view cannot dispatch while it is evaluated.

The input has the SpinButton role with the numeric value, range and step.
Assistive values are `f32`, so they lose precision beyond about seven
significant digits, and an unbounded end publishes `f32::MIN` or `f32::MAX`.
`.disabled(true)` disables the input and stepper. `.read_only(true)`, or a
field without `on_change`, keeps the value selectable and focusable but
rejects edits, steps and assistive changes.

The draft and stepper state belong to the call site in the current placement,
so the builder takes `cx`; fields built in a loop need
`number_field_keyed(cx, key, value)`.

## Disclosures

```rust
disclosure(cx, "Advanced", column().gap(8.)
    .child(checkbox(self.trim, "Trim trailing whitespace")
        .on_change(cx.listener(|s, e: &ToggleEvent, _| s.trim = e.checked))))
```

The header is a quiet button with a chevron, right when collapsed and down when
expanded, and the title. A click, Enter or Space toggles it. Without
`.expanded(bool)` the disclosure keeps its own state in the placement and
`.on_toggle(listener)` only reports changes. With it, the application owns the
state and `on_toggle` proposes `ToggleEvent { checked }`.

Expanding reveals the content. Its height grows from zero to its natural height
and it fades in, over the theme's `overlay_enter` duration. Collapsing reverses
that. The content region is a vertical scroll container with a scroll handle,
so its viewport height plus its scroll range is the content's natural height,
read after the first layout. While the height moves the region has a fixed
height; once settled it follows its content. With reduced motion, or without a
host frame clock, the content shows and hides at once.

Collapsing content is inert immediately: it leaves Tab order and the
accessibility tree before its motion ends, and is removed when the motion ends.
The header publishes its expanded state and, while the content shows, controls
the content's Group, which the title names.

## Callouts

```rust
callout(Tone::Warning, "rust-analyzer is out of date")
    .description("Some features may not work until it is updated.")
    .action(button("Update"))
```

A callout is a row: the tone's icon, a medium-weight title, an optional muted
`.description(text)`, the `.action(element)` elements in the order they were
added, and a close button when `.on_dismiss(listener)` is set. It fills its
parent's width.

| Tone | Icon | Role |
| --- | --- | --- |
| `Neutral` | `INFO` | Status |
| `Accent` | `INFO` | Status |
| `Danger` | `CIRCLE_ALERT` | Alert |
| `Warning` | `TRIANGLE_ALERT` | Alert |
| `Success` | `CIRCLE_CHECK` | Status |

The fill is the tone's subtle background, with a 1-unit border in the tone's
foreground at half alpha and the theme's `radius`. The icon uses the tone's
foreground.

## Toasts

```rust
toasts(cx, &self.toasts)
    .on_dismiss(cx.listener(|s, e: &ToastDismiss, _| s.toasts.retain(|t| t.id != e.id)))
    .on_action(cx.listener(|s, e: &ToastAction, _| s.retry(e.id)))
```

The application owns a `Vec<Toast>`. `Toast::new(id, title)` is neutral with a
5-second timeout; `.tone(..)`, `.description(..)`, `.action(caption)` and
`.timeout(None)` change it. Attach `toasts` anywhere in the window. It takes no
space.

Each toast is a passive corner overlay: a raised card at `Elevation::Popover`
with the theme's `radius_large`, 360 units wide. It shows the tone's icon, the title, the
description, an optional action button and a close button. Toasts stack in
`.position(ToastPosition::..)`, `BottomEnd` by default, 16 units from the window
edges with 8 units between them. The last toast in the list is nearest the
corner. They enter and exit with the theme's overlay motion, rising from the
bottom edge or dropping from the top, and appear at once with reduced motion.

Toasts never take focus. A corner overlay does not block the window behind it:
only its cards are pointer targets, and outside presses and Escape stay with the
window. Tab reaches the toast buttons in the position of `toasts` in the view.

A toast's timeout runs on the frame clock through view deadlines. It starts on
the first fresh frame after the toast appears, and pauses while the pointer is
over any toast or focus is in one. When it passes, the stack proposes
`ToastDismiss { id }` once, at the runtime's next flush. The close button
proposes the same event, and the action button proposes `ToastAction { id }`.
The application removes the toast to accept a dismissal.

A polite live region, which is not drawn, announces each new toast's title and
description once. Removing toasts does not repeat an earlier announcement.

## Verification

The tests in `crates/rxui/src/standard/forms_tests.rs` and
`crates/rxui/src/standard/feedback_tests.rs` cover label wiring, descriptions,
the invalid state and border for every control type, number parsing, clamping,
precision, keys, commit and revert, stepper repeats, disabled and read-only
fields, the AccessKit SpinButton mapping, disclosure toggling, motion and
inertness, callout paint and roles, toast expiry, pausing, stacking, input
pass-through and the live region. The gallery's Forms and Feedback sections show
them all:

```sh
cargo run -p rxui --example gallery --features native
```
