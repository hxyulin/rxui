# Standard widgets: design

Status: implemented. Icons are in [icons.md](icons.md), small parts, toolbars
and choices in [controls.md](controls.md), menu items in
[overlays-and-commands.md](overlays-and-commands.md), comboboxes and list views
in [search-and-lists.md](search-and-lists.md), and forms and feedback in
[forms-and-feedback.md](forms-and-feedback.md).
This file keeps the choices that differ from the agreed sketch, the open items
and the widgets planned for later.

## Principles

- **Controlled.** The application owns every value and receives typed
  proposals through a `Listener`, like `select`, `slider` and `tabs`. Transient
  interaction state that is not application data (open, highlight, timers)
  belongs to the placement through `ViewContext::use_keyed_state`, so those
  builders take `cx` and have a `_keyed` variant for loops.
- **Composed from existing primitives.** Buttons supply focus, Tab stops,
  activation and hover/pressed/disabled paint. Popovers supply placement and
  dismissal. `list_row`, theme tokens, text roles, elevation and density supply
  the look. A new widget should not need new painting code unless it draws
  vector content.
- **Themed and density-aware.** Sizes come from `ThemeMetrics`; colors from
  `ThemeColor` tokens. Nothing hard-codes a color, and compact density shrinks
  everything consistently.
- **Accessible.** Every widget has a real `SemanticRole`, an accessible name,
  and the platform keyboard pattern for that role.
- **General-purpose.** RXUI is used by a Rust IDE, but IDE features (editor,
  command palette, quick open) live in the application and are composed from
  these widgets.

## Batch 1: foundations

Implemented. The contracts are in [icons.md](icons.md) (icons, icon placement
and icon buttons), [controls.md](controls.md#separators) (`Tone`, `separator`,
`badge` and `kbd`) and
[overlays-and-commands.md](overlays-and-commands.md#focus-menus-and-modal-boundaries)
(menu item icons, shortcuts, danger items and `menu_separator`).

## Batch 2: toolbar and choices

Implemented. The contracts are in [controls.md](controls.md#segmented-controls)
(`segmented` and `Choice::icon`), [controls.md](controls.md#toggle-buttons)
(`toggle_button`) and [controls.md](controls.md#toolbars) (`toolbar`). Two
choices differ from the agreed sketch: `segmented()` takes no view context,
because it holds no placement-local state, and its arrow keys stop at the ends
so a toolbar around it receives them.

## Batch 3: search and lists

Implemented. The contracts are in [search-and-lists.md](search-and-lists.md)
(`combobox`, matchers and `list_view`), and `SelectionMode` is shared with
[virtual trees](virtual-trees.md). Three choices differ from the agreed sketch.
The built-in matchers are unit structs, `FuzzyMatcher`, `SubstringMatcher` and
`PassthroughMatcher`, because `Matcher` is a trait and cannot carry
constructors. Matched text uses `Focus` rather than `Accent`, which is not
readable as text. The highlight follows an item's index, since items have no
keys.

## Batch 4: forms and feedback

Implemented. The contracts are in [forms-and-feedback.md](forms-and-feedback.md)
(`field`, `number_field`, `disclosure`, `callout` and `toasts`), and
`accessibility_invalid` is in [semantics.md](semantics.md). Four choices differ
from the agreed sketch. `number_field` and `toasts` take the view context,
because the draft text, stepper repeats and toast clocks are placement-local.
Number fields propose a `NumberPhase` of their own rather than `SliderPhase`,
whose drag phases do not apply. A toast's action is a caption reported through
`Toasts::on_action`, so the application's list holds data rather than elements.
Callouts take `Listener<ClickEvent>` for `on_dismiss`.

Open items:

- A field shows its error as the control's description; AccessKit's
  `error_message` relation is not published.
- A toast card is 360 units wide even in a narrower window.

## Later

Data table or grid, color picker, breadcrumbs, date picker, toolbar overflow
menu.
