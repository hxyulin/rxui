# Standard widgets: design

Status: agreed design. Batch 1a (icons, icon placement and icon buttons) is
implemented; see [icons.md](icons.md). Widgets land in four batches; this document
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

### Icons

Implemented as batch 1a. The contract is in [icons.md](icons.md). Built-in icons
are associated constants such as `Icon::SEARCH`.

### Small parts

```rust
separator()                         // orientation follows a row/column parent, or .vertical()
badge("3 errors").tone(Tone::Danger)
kbd(Shortcut::primary("P").shift()) // ⌘⇧P on macOS, Ctrl+Shift+P elsewhere
```

- `Tone { Neutral, Accent, Danger, Warning, Success }` maps to foreground and
  subtle-background tokens. Badges, callouts, toasts and field errors share it.
- `separator` uses `Divider`, has the `Separator` role, and is not focusable.
- `kbd` renders a `Shortcut` with platform conventions: modifier symbols on
  macOS, `Ctrl+Shift+P` elsewhere. It has caption size, muted text, and a
  bordered key-cap look.

### Menu items

`menu_item(action)` gains an optional leading icon (`.icon(Icon::FILE)`) and
shows the command's shortcut automatically as a trailing `kbd`. Items in one menu
reserve the icon column when any item has an icon, so captions align.

## Batch 2: toolbar and choices

```rust
toolbar()
    .child(icon_button(Icon::PLUS).accessibility_label("New"))
    .child(separator())
    .child(toggle_button(self.wrap, Icon::TEXT_WRAP).accessibility_label("Wrap")
        .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked)))

segmented(cx).selected(self.view.clone()).accessibility_label("Diff view")
    .option(choice("split", "Split"))
    .option(choice("unified", "Unified"))
    .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.view = e.key.clone()))
```

- `toolbar` has the `Toolbar` role and a single Tab stop. Left/Right (or
  Up/Down when vertical) and Home/End move focus between its items; focus is
  remembered when Tab re-enters. It is built on the existing focus groups.
- `toggle_button(pressed, content)` is a Quiet button with the `ToggleButton`
  role (pressed state). Pressed shows the `RowSelected` fill. Content is text,
  an `Icon`, or any element.
- `segmented` reuses `Choice` and `ChoiceEvent`. It is a radio group visually
  joined into one control: an inset track with the selected segment raised.
  Arrow keys move and select, like radio groups. Segments take text or icons
  (`choice(..).icon(Icon::LIST)`).

## Batch 3: search and lists

```rust
combobox(cx)
    .query(&self.query)
    .on_query(cx.listener(|s, e: &QueryEvent, _| s.query = e.text.clone()))
    .items(self.files.len(), |i| ComboItem::new(&self.files[i].name)
        .icon(Icon::FILE).detail(&self.files[i].dir))
    .matcher(Matcher::fuzzy())
    .on_choose(cx.listener(|s, e: &ChooseEvent, _| s.open(e.index)))
```

- `combobox` is a text input with a results list in a popover, or inline
  (`.inline()`) for palette-style dialogs. Focus stays in the input. Up/Down,
  PageUp/PageDown, Home/End move the highlight, Enter chooses and Escape closes.
  It uses the `ComboBox` and `ListBox` roles with an active-descendant
  highlight.
- Results are virtualized (`virtual_list`), so large item sets stay cheap.
- `Matcher` is a trait: `fn score(&self, query: &str, candidate: &str) ->
  Option<Match>`, where `Match` carries a score and matched character ranges.
  `Matcher::fuzzy()` is a built-in subsequence matcher that ranks word starts,
  consecutive runs and case. `Matcher::substring()` and `Matcher::none()` (the
  application filters) are also provided. Matched ranges are highlighted with
  rich-text spans in `Accent`. Matching runs on the UI thread; applications
  with huge sets filter themselves and use `Matcher::none()`.
- `list_view(cx)` is a selectable, virtualized list with `Single` or
  `Multiple` selection, built on `list_row`. Keyboard: arrows,
  Home/End/PageUp/PageDown, Shift-extend, Primary-toggle, type-ahead. The
  selection model (anchor, focus, extend, toggle) moves out of `virtual_tree`
  into a shared type used by both.

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
