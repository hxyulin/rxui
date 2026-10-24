# Standard widgets: design

Status: agreed design. Batches 1 to 3 are implemented: icons in
[icons.md](icons.md), small parts, toolbars and choices in
[controls.md](controls.md), menu items in
[overlays-and-commands.md](overlays-and-commands.md), and comboboxes and list
views in [search-and-lists.md](search-and-lists.md). Widgets land in four batches; this document
records the intended API and behavior so each batch builds on the same contract. Once a
widget is implemented, its contract moves to the matching per-area document
(usually [controls.md](controls.md)); this file then keeps only open items.

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

```rust
field("Tab width", number_field(self.tab).range(1., 16.).step(1.)
        .on_change(cx.listener(|s, e: &NumberEvent, _| s.tab = e.value)))
    .help("Spaces per indent level.")
field("Font family", text_input(&self.font)).error(self.font_error.as_deref())
disclosure(cx, "Advanced", content)
callout(Tone::Warning, "rust-analyzer is out of date")
    .description("Some features may not work until it is updated.")
    .action(button("Update"))
toasts(&self.toasts).on_dismiss(cx.listener(|s, e: &ToastDismiss, _| s.remove(e.id)))
```

- `field` stacks a label, the control, and help or error text. It wires
  `accessibility_labelled_by` and the description automatically. An error
  switches the control's border to `Danger` and announces the message.
- `number_field` is a text input with a stepper: range, step, precision,
  Up/Down and PageUp/PageDown keys, and commit on Enter or blur. Invalid text
  reverts. It proposes `NumberEvent { value, phase }` like the slider.
- `disclosure` is a header button with a chevron icon and `expanded` state, and
  an animated content reveal that respects reduce-motion.
- `callout` is an inline banner with tone, icon, title, optional description
  and actions.
- `toasts` render an application-owned list (`Toast { id, tone, title,
  description, action, timeout }`) as a corner stack overlay with
  enter/exit motion. Timers pause on hover and keyboard focus. The polite live
  region announces new toasts.

## Later

Data table or grid, color picker, breadcrumbs, date picker, toolbar overflow
menu.
