# Standard form controls

Status: check boxes, switches, radio groups, segmented controls, toggle buttons,
toolbars, sliders, progress bars, spinners, selects, hover/keyboard-focus
tooltips, separators, badges and key caps are implemented with the default
layout feature.
`rendering` paints their vector marks; `accessibility` publishes them through
AccessKit. Every control is composed from existing primitives, so focus, input
routing, theming and semantics follow the same contracts as buttons and popovers.

## Application API

```rust
use rxui::prelude::*;

struct Settings { wrap: bool, sync: bool, endings: Key, zoom: f32, language: Key }
impl View for Settings {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().gap(10.)
            .child(checkbox(self.wrap, "Wrap lines")
                .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked)))
            .child(switch(self.sync, "Background sync")
                .on_change(cx.listener(|s, e: &ToggleEvent, _| s.sync = e.checked)))
            .child(radio_group().selected(self.endings.clone())
                .accessibility_label("Line endings")
                .option(choice("lf", "LF"))
                .option(choice("crlf", "CRLF"))
                .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.endings = e.key.clone())))
            .child(slider(self.zoom).range(50., 200.).step(10.)
                .accessibility_label("Zoom")
                .on_change(cx.listener(|s, e: &SliderEvent, _| s.zoom = e.value)))
            .child(select(cx).selected(self.language.clone())
                .accessibility_label("Language")
                .option(choice("rust", "Rust"))
                .option(choice("toml", "TOML"))
                .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.language = e.key.clone())))
            .child(tooltip(cx, button("Format"), "Reformat the document"))
    }
}
```

Values are controlled. The application owns each value and receives a typed
proposal: `ToggleEvent { checked }` from check boxes, switches and toggle
buttons, `ChoiceEvent { key }` from radio groups, segmented controls and selects, and
`SliderEvent { value, phase }` from sliders. Storing the proposal accepts it;
ignoring or replacing it rejects or normalizes it. A control without a listener
still focuses and publishes its state but cannot change.

The builders return `Toggle`, `ToggleButton`, `RadioGroup`, `Segmented`,
`Toolbar`, `Slider`, `ProgressBar` and `Select`.
Each accepts `.key(...)`, `.accessibility_label(...)`,
`.accessibility_description(...)`, `.width(...)`, `.fill_width()`, `.flex_grow(...)`,
`.min_width(...)` and `.layout(...)` for its root, plus `.disabled(true)` where
input applies. Call `.into_element()` for any other element builder. `spinner(cx)`
and `tooltip(cx, ...)` return an ordinary `Element`.

Options are `choice(key, caption)`, optionally `.disabled(true)` and
`.icon(Icon)`. Only segmented controls show the icon. Keys are unique
within one group or select; duplicates fail preparation with
`UiError::DuplicateKey`. A selected key that names no option leaves a radio group
unchecked and shows a select's placeholder. Slider and progress ranges must be
finite with `min <= max`, and a slider step must be positive; invalid ranges fail
preparation with `UiError::InvalidStyle` rather than panicking.

## Check boxes and switches

`checkbox(checked, caption)` accepts a `bool` or `Checked`. `Checked::Mixed` paints
a bar, is published as mixed, and proposes `true` when activated, so a parent box
can drive its children. `switch(on, caption)` is an on/off toggle with the Switch
role. Both are quiet buttons: pointer clicks, Enter/Space and assistive Click use
the ordinary button activation path. The caption is the accessible name; a control
without a caption needs `.accessibility_label(...)`.

## Radio groups

`radio_group()` holds options in navigation order; `.axis(Axis::Horizontal)` lays
them out in a row. The group is one Tab stop: the selected enabled option, or the
first enabled option without a selection. Arrow keys in either orientation move
focus to the next or previous enabled option, wrapping and skipping disabled
options, and propose it immediately, like the platform radio group. Space, Enter,
clicks and assistive Click propose the activated option. Arrow keys with any
modifier are left to other handlers. Each option publishes the RadioButton role,
its checked state, its zero-based position and the group size; the group is a named
RadioGroup.

Focus moves through a fresh `FocusHandle` per option, queued during the key event
and applied at the next preparation, so the new option is focused even when the
application rejects the proposal.

## Segmented controls

```rust
segmented().selected(self.view.clone()).accessibility_label("Diff view")
    .option(choice("split", "Split"))
    .option(choice("unified", "Unified"))
    .option(choice("raw", "Raw"))
    .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.view = e.key.clone()))
```

`segmented()` is a radio group drawn as one control. The options sit in an
inset track as tall as a text input: the Input fill, a 1-unit InputBorder
outline with the control radius, and 1 unit of padding. Segments are 4 units
shorter than the track, with a radius 2 units smaller. The selected segment is
raised like a default button: the Control fill, a Border outline, the control
shadow and Text. The others are quiet, with TextMuted and a RowHover fill while
hovered. `choice(..).icon(Icon::LIST)` shows only the icon in a square segment,
and the caption becomes its accessible name.

Segments share the width equally by default: in a content-sized track each is
as wide as the widest, and a stretched track divides its width. The track is a
grid of equal fractions. `.equal_width(false)` sizes each segment to its
content. Compact density shrinks the track and segments with `input_height`.

Semantics and keys follow radio groups: a RadioGroup of RadioButtons with their
checked state, position and set size, and one Tab stop. Arrow keys move to the
previous or next enabled segment and propose it. Unlike a radio group, they
stop at the ends instead of wrapping; there the key is not handled, so a
segmented control inside a toolbar passes it to the toolbar. Disabled options
are skipped. `segmented()` takes no view context: like `radio_group()`, it holds
no placement-local state.

## Toggle buttons

```rust
toggle_button(self.wrap, Icon::TEXT_WRAP).accessibility_label("Wrap lines")
    .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked))
```

`toggle_button(pressed, content)` is a quiet button that stays pressed. Content
is `impl Into<ToggleContent>`: a `&str` or `String` caption, an `Icon`, or any
`Element`, such as `label("Wrap").leading_icon(Icon::TEXT_WRAP)`. An icon
toggle is an icon button, `input_height` square, and needs an accessible name;
preparation fails with `UiError::MissingAccessibleName` without one.

Unpressed, it looks like a Quiet button. Pressed, it has the RowSelected fill
with Text content, and keeps the fill while disabled. AccessKit has no toggle
button role, so it publishes the Button role with a check state. Clicks,
Enter, Space and assistive Click propose `ToggleEvent { checked: !pressed }`.

## Toolbars

```rust
toolbar().accessibility_label("Editor")
    .child(icon_button(Icon::PLUS).accessibility_label("New file"))
    .child(separator())
    .child(toggle_button(self.wrap, Icon::TEXT_WRAP).accessibility_label("Wrap lines")
        .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked)))
    .child(button("Run").leading_icon(Icon::PLAY).variant(ButtonVariant::Primary))
```

`toolbar()` is a row with the Toolbar role and a horizontal orientation. Its
items are buttons, icon buttons, toggle buttons, selects, segmented controls and
separators, vertically centered with a 4-unit gap. The toolbar is at least
`input_height` tall. `.vertical()` stacks the items in a column at least
`input_height` wide, with a vertical orientation.

The toolbar is one Tab stop. Tab enters at the item focused last, by keyboard or
pointer. Without one it enters at the first item, or the last with Shift-Tab.
The next Tab leaves the toolbar from any item. Left and Right (Up and Down when
vertical) move focus to the previous or next item and wrap. Home and End move it
to the first and last item. Separators and disabled items are skipped, and the
keys need no modifiers. An item that handles an arrow key itself keeps it: a
select uses Up and Down, and a segmented control moves between its segments
until it reaches an end. Arrow keys in a text input, or in a popover opened from
an item, never move toolbar focus.

## Sliders

`slider(value)` defaults to a 0..=1 continuous range, 200 logical units wide.
`.range(min, max)`, `.step(step)` and `.page_step(page)` configure it. Values are
clamped to the range and snapped to the step before they are proposed.

| Input | Proposal and phase |
| --- | --- |
| Primary press | Captures the pointer, focuses the slider and jumps to the pointer: `Begin` |
| Captured motion | Follows the pointer horizontally, anywhere in the window: `Drag` |
| Release | Final pointer value: `End` |
| Escape, host cancellation or removal | Repeats the current value: `Cancel` |
| Right/Up, Left/Down | One step (a hundredth of the range without a step): `Keyboard` |
| PageUp, PageDown | One page, a tenth of the range or one step, whichever is larger: `Keyboard` |
| Home, End | Minimum, maximum: `Keyboard` |
| Assistive SetValue, Increment, Decrement | Clamped and snapped value: `Accessibility` |

Drag and key proposals equal to the current value are not sent. Cancellation does
not roll back: the slider does not retain the value from before the press, so an
application that wants Escape to restore it stores the value it held before `Begin`.
The slider publishes the Slider role with its value, range and step, and supports
AccessKit SetValue/Increment/Decrement through `on_semantic_action`.

## Progress bars and spinners

`progress_bar(value)` takes `f32` or `Option<f32>`. A value shows determinate
progress through `.range(min, max)` (0..=1 by default) and publishes a numeric
ProgressIndicator. `None` shows an indeterminate segment and publishes the value
text "Indeterminate". `.animate(cx)` sweeps that segment from the view's frame
clock; it calls `request_animation_frame` on every evaluation while the bar is
indeterminate and does nothing for a determinate bar. With reduced motion enabled,
it shows a centered static segment and requests no frames.

`spinner(cx)` is a 20-unit activity indicator named "Loading", painted in the
accent color. It requests the next frame unless reduced motion is enabled; then
it keeps a static spoke pattern. Both animations re-evaluate the
owning view once per frame while shown, so put them in a small component when the
surrounding view is expensive, and remove them when the work finishes. They use
the existing frame clock and request; they are not an animation system.

## Selects

`select(cx)` is a ComboBox button showing the current option's caption, or the
`.placeholder(...)` text in a muted color, with a chevron. Activating it opens a
ListBox popover anchored below it, flipped above when there is no room. Focus stays
on the button while the list is open, as in the platform combo box pattern; the
highlighted option is placement-local state keyed by the option key. Opening on a
selection and moving the highlight reveal the option through the popup viewport.
Reordering preserves highlight identity; removal or disabling falls back to the
enabled selection or the first enabled option. An empty/all-disabled list has no
active descendant.

| State | Keys |
| --- | --- |
| Closed | Up/Down open the list on the selection. Enter/Space open it through ordinary activation. A letter proposes the next enabled option whose caption starts with it. |
| Open | Up/Down move the highlight without wrapping, Home/PageUp and End/PageDown jump to the ends, a letter moves to the next match, Enter/Space choose the highlight and close, Tab closes and moves focus, Escape closes. |

Typing within one second extends a case-insensitive prefix in either state.
Repeated single letters cycle matches; a prefix with no match starts a new search
from the latest input. Opening/closing clears the prefix.

Pointer hover highlights an option and a click chooses it. An outside press closes
the list and is consumed, like other popovers. Disabled options are painted muted
and cannot be highlighted or chosen. Choosing closes the list and proposes the key,
including the current one. The button publishes ComboBox with expanded state and
the current caption as its value; `.accessibility_label(...)` names both the button
and the list. The button publishes `controls` for its popup ListBox and
`active_descendant` for the enabled highlight while open; closing removes both
relationships without moving focus. Options publish ListBoxOption with selected state, position and set
size. The list is as wide as a fixed `.width(...)`, else 200 units; use
`.list_width(...)` for selects that fill their parent.

Open state belongs to the call site in the current placement, through
`ViewContext::use_keyed_state`. Two selects created by the same line, such as in a
loop or a helper function, need `select_keyed(cx, key)` with distinct keys.

## Tooltips

`tooltip(cx, target, text)` wraps any element. After the pointer rests on the
target, or keyboard focus rests in its subtree, for `TOOLTIP_DELAY` (600 ms) a small popover shows the text below the target,
flipped above when there is no room. It hides when neither hover nor keyboard focus remains, on native deactivation, on any
press or key press inside the target, on Escape, and when the target is hidden or
scrolled out of view; a hidden tooltip returns after the pointer leaves and enters
again. Disabled targets still show their tooltip.

A tooltip never takes focus and never intercepts input. Its overlay has
`PointerEvents::None`, so the pointer reaches whatever is beneath it, and outside
presses are not consumed: the click that hides a tooltip still activates the
control. The text is published as the target's accessible description unless it
already has one, and the popover is hidden from assistive technology.

The wrapper takes over the target's flex and grid item properties (grow, shrink,
basis, alignment, margins, insets, position, grid placement and percentage
sizes) so the target keeps its place in the parent layout. Hover state is
placement-local, keyed by call site, the target's key and the text; tooltips built
in a loop should wrap keyed targets. The delay counts frames from the host clock:
the first fresh frame after the pointer enters starts it, and the owning view is
evaluated once per frame until the tooltip shows.

## Separators

```rust
row().gap(4.).child(button("Cut")).child(separator()).child(button("Find"))
column().child(label("General")).child(separator()).child(label("Advanced"))
```

`separator()` is a 1-unit Divider line with the `Separator` role. It is not
focusable. Its orientation follows its flex parent without inspecting it: a
1-unit flex basis sizes the line along the parent's main axis. In a column it is
a horizontal line across the content width. In a row it is a vertical line
`ThemeMetrics::icon_size` tall (16 units balanced, 14 compact), centered.

`.horizontal()` fixes a 1-unit line that stretches across a column, minus its
margins, so `.horizontal().margin_x(8.)` insets it. `.vertical()` fixes a 1-unit
wide, icon-tall line; give it a height for a longer one. Use one of them outside
a flex row or column, such as in a stack. `.margin(...)`, `.margin_x(...)` and
`.margin_y(...)` add spacing, and the root builders above also apply.

## Badges

```rust
row().gap(6.)
    .child(badge("Draft"))
    .child(badge("3 errors").tone(Tone::Danger))
```

`badge(text)` is a pill of medium-weight Caption text with 1 unit of vertical and
7 units of horizontal padding. It is not focusable. `.tone(Tone)` picks its
colors; the default is `Tone::Neutral`.

`Tone` is shared with the later callouts, toasts and field errors:

| Tone | Foreground | Background |
| --- | --- | --- |
| Neutral | TextMuted | ControlHover |
| Accent | Focus | AccentSubtle |
| Danger | Danger | DangerSubtle |
| Warning | Warning | WarningSubtle |
| Success | Success | SuccessSubtle |

Neutral uses the hovered control fill because the resting Control fill equals
the light theme's surface, which would hide the pill. Accent uses the Focus
color as its foreground: it is the accent's hue adjusted to 4.5:1 against the
window surfaces and the accent tint, which the accent fill does not reach.
Every tone's text reaches 4.5:1 on the background, surface and raised fills,
bare and under its tint.

A badge is a label, so a button or tab containing it includes its text in its
accessible name: a tab captioned "Problems" with a badge "3" is named
"Problems 3". Hide it with `.accessibility_hidden(true)` when the parent's name
already says what it shows.

## Key caps

```rust
row().gap(6.).child(label("Command palette")).child(kbd(Shortcut::primary("p").shift()))
```

`kbd(shortcut)` shows a `Shortcut` as Caption text in TextMuted on the
Background fill, inside a 1-unit Border outline with the theme's small radius.
Its text is `Shortcut::label()`. On macOS that is the modifier symbols in the
order ⌃⌥⇧⌘ followed by the key, such as `⇧⌘P`. Elsewhere it is
`Ctrl+Alt+Shift+Super+P` with the modifiers held. Character keys are uppercase.
Named keys use symbols on macOS (`↩`, `⎋`, `⇥`, `⌫`, `⌦`, arrows) and words
elsewhere (`Enter`, `Esc`, `Tab`, `Backspace`, `Del`, `Left`). Function keys are
`F1`, `F2` and so on in both styles. Assistive technology reads the text of a standalone key
cap. [Menu items](overlays-and-commands.md) show their command's shortcut with a
key cap and publish it as a keyboard shortcut instead.

## Theming

Controls use semantic tokens only, so they follow `Theme::dark()`,
`Theme::light()` and `Theme::high_contrast()` and switch live. Captions inherit
the button foreground, including the disabled text color.

| Part | Resting | Checked or filled | Disabled |
| --- | --- | --- | --- |
| Check box, radio, switch indicator | Input fill, TextMuted outline and switch knob | Accent fill, AccentText mark | ControlDisabled with BorderDisabled; TextDisabled when checked |
| Slider rail and thumb | Input fill, TextMuted outline | Accent fill | ControlDisabled, BorderDisabled, TextDisabled fill |
| Progress track | Input fill, TextMuted outline | Accent fill | (not interactive) |
| Select list | Raised popover with a Divider outline and popover shadow | RowHover for the highlight, a check for the selection | TextDisabled options |
| Tooltip | Raised popover, Caption text | | |
| Segmented track | Input fill, InputBorder outline; TextMuted segments | Control fill, Border outline, control shadow, Text | TextDisabled segments |
| Toggle button | Quiet: no fill, TextMuted | RowSelected fill, Text | TextDisabled, keeping the pressed fill |

Rails and tracks are outlined rather than filled with the border color so the
accent fill stays distinct in high contrast, where Border is white and the accent
is yellow. Input borders are soft boundaries below 3:1 in light and dark, so
unchecked indicators, rails and tracks outline themselves with TextMuted. Tests
verify for every preset that the accent has at least 3:1 contrast against
Background, Surface and the Input fill, that the unchecked outline and switch knob
have at least 3:1 against Input and Surface, and AccentText at least 4.5:1 on the
accent. Dark's accent on the Raised popover fill is 2.96:1, recorded rather than
hidden.

Indicators follow the theme's metrics: `indicator_size` sizes check boxes and
radios (14 units balanced, 12 compact), and `switch_width` × `switch_height`
sizes switches (26×16, 22×13). The slider thumb is as tall as a switch, its rail
three eighths of that, and progress bars half (8 units balanced, 6 compact; an
explicit `.height` wins). Marks and dots scale with their box, and a theme or
density switch resizes them live. Captioned rows stay as tall as a button. The
slider's thumb travel stays 8 units inside its ends at every density, so a
compact thumb stops just short of them. See
[styling](styling.md#default-appearance-and-accessibility-targets).

![Standard controls in the dark theme, with a tooltip shown](images/controls-dark-validation.png)
![Standard controls in the light theme, with the select list open](images/controls-light-validation.png)
![Standard controls in the high-contrast theme, with the select list open](images/controls-high-contrast-validation.png)

## Shared files and internals

Controls live in `crates/rxui/src/standard/`. Their internal listeners run outside
any component scope. Two core additions support them. A crate-private
`FocusHandle::focus_bound` queues focus for a handle's single live binding, so
radio and segment arrow keys can move focus without a source mount. A
crate-private roving focus group, set on a toolbar's root, builds on
`FocusScope::Group` memory: Tab traversal skips the group's other Tab stops while
focus is inside it, and `Ui::key` moves focus between them as a default after
listeners, like tab headers. The check box tick,
mixed dash and spinner are a crate-private `CustomElement` painted with rxui-draw
lines in the element's resolved text color, so they need no font coverage. The
select chevron and the selected option's check are [icons](icons.md).

## Limitations and follow-ups

- Select type-ahead uses a one-second case-insensitive prefix, without locale-aware
  collation or an editable search field. Native screen-reader acceptance of popup
  highlight announcements still needs testing on each platform.
- Sliders are horizontal only and have no tick marks, value bubble or vertical
  orientation. Cancel does not restore the value from before the press.
- Spinners and animated indeterminate bars re-evaluate their owning view every
  frame unless reduced motion is enabled.
- Indicators and paddings do not follow compact metrics.
- Tooltip delay, placement and the select list's maximum height are fixed.
- No tooltip "warm-up" (an immediate tooltip when moving between targets).

## Verification

Headless tests cover pointer, keyboard (with the host's Enter/Space activation
default) and assistive activation of check boxes and switches; mixed and disabled
state and Tab exclusion; radio roving Tab stops, arrow focus movement that wraps
and skips disabled options, and group semantics; slider capture, step snapping,
every key, assistive SetNumericValue, Escape cancellation and invalid ranges;
progress values, indeterminate frame requests, spinner semantics and preference
toggling in both directions; select opening without moving focus, viewport reveal
for long lists, highlight identity through reorder/removal, active-descendant and
controls relationships, timed multi-character type-ahead in both states,
Enter/Escape/Tab, hover and click choice, disabled options and consumed outside
presses; tooltip delay by frame clock, focus preservation, pointer transparency,
press/Escape/leave hiding and description publication; tooltip layout adoption;
and preset contrast. Toolbar tests cover arrow keys, Home and End, wrapping,
skipped separators and disabled items, the hand-off from a segmented control,
Tab entering once and remembering the last item from keys and clicks, the
vertical orientation, layout and semantics. Toggle button tests cover the
pressed and unpressed paint in light and dark, the pressed state, proposals and
the name requirement for icon toggles. Segmented tests cover the track and
segment paint, arrow keys that select and stop at the ends, disabled segments,
semantics, equal and content widths, icon segments and compact sizing. Separator tests cover orientation in rows and columns at
both densities, the fixed orientations with margins, the role and Tab exclusion.
Badge tests cover each tone's tokens in every preset, the caption pill and the
parent name. Shortcut tests cover modifier order, named keys, function keys and
both platform styles; key cap tests cover the paint. A theme test checks each
tone's contrast in every preset and generated theme. An AccessKit test checks roles, toggled state, numeric step
and actions, expanded state, and Increment/Click round trips. An opt-in GPU test
reads back indicator, tick, switch knob, slider and progress pixels in all three
presets; another reads back the segmented track, the raised segment and a pressed
toggle's fill.

```sh
cargo test -p rxui --all-features standard:: --locked
cargo test -p rxui --features rendering standard::gpu_tests --locked -- --ignored
cargo run -p rxui --example gallery --features native --locked
```

The screenshots above were rendered headlessly through `TestUi` at raster scale one
with the system UI font, and inspected visually. They are not native window captures.
