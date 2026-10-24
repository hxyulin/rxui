# Icons

Status: vector icons, icon placement in buttons and labels, and icon buttons are
implemented with the default layout feature. `rendering` paints them through
rxui-draw paths.

## Application API

```rust
use rxui::prelude::*;

row().gap(8.)
    .child(icon(Icon::SEARCH))                                    // decorative
    .child(icon(Icon::CIRCLE_ALERT).accessibility_label("Error")) // meaningful
    .child(button("Run").leading_icon(Icon::PLAY).variant(ButtonVariant::Primary))
    .child(button("More").trailing_icon(Icon::CHEVRON_DOWN))
    .child(icon_button(Icon::SETTINGS).accessibility_label("Settings"))
```

`Icon` is a handle to shared, parsed geometry in a view box, plus a paint mode.
Cloning it copies a pointer. Icons compare by value, so a view that rebuilds the
same icon does not invalidate layout or paint.

- Built-in icons are associated constants such as `Icon::SEARCH`. Each one is
  parsed on first use and then shared. `Icon::built_in()` lists them and
  `Icon::name()` returns the Lucide name.
- `Icon::from_svg_path(view_box, d)` parses the `d` attribute of an SVG
  `<path>`. It returns `IconError` with the byte offset of malformed data, or for
  a view box that is not finite and positive. Two icons parsed from equal data
  are equal.
- `Icon::from_path(view_box, path, paint)` (with `rendering`) wraps a prebuilt
  `rxui_draw::Path`. It equals only its own clones.
- Icons stroke by default. `.fill()` fills closed contours with the nonzero rule
  instead, and `.stroke()` switches back.

## Path data

The parser supports the `M L H V C S Q T A Z` commands in absolute and relative
forms. It accepts implicitly repeated arguments, comma or whitespace separators,
exponents, and compact numbers: `1.5.5` is two numbers and so is `-1-2`. Arc
flags need no separator. `S` and `T` reflect the previous curve's control point.
Arcs become cubic Béziers of at most a quarter turn each, with radii that are too
small scaled up as SVG specifies. Drawing after `Z` starts a new contour at the
closed contour's start. Full SVG documents, transforms and other shape elements
are out of scope: convert them to path data first.

## Painting and size

`icon(icon)` paints with the element's resolved text color. It follows hover,
pressed, disabled and theme switches like text does, and `.color(...)` overrides
it. A stroke is one twelfth of the view box's smaller side wide (2 units in a
24-unit box) with round caps and joins, so it scales with the icon.

The icon is `ThemeMetrics::icon_size` square: 16 units balanced, 14 compact. The
size resolves with the theme, so a density switch resizes icons live. An explicit
`.size(...)`, `.width(...)` or `.height(...)` wins. The geometry scales uniformly
to fit the content box and centers in it. Tessellation is retained per placement
and rebuilt only when the icon changes or its scale needs a finer curve
tolerance.

## Placement

`.leading_icon(icon)` and `.trailing_icon(icon)` add an icon before or after a
caption. On a button the content becomes a centered row with a 6-unit gap, and
the caption keeps naming the button. A label or rich text becomes a centered row
of icon and text that keeps the label's key.

`icon_button(icon)` is a Quiet button as tall and wide as a text input
(`ThemeMetrics::input_height`) with the icon centered. It needs an accessible
name from `.accessibility_label(...)` or `.accessibility_labelled_by(...)`.
Without one, preparation fails with `UiError::MissingAccessibleName`. Wrap it in
`tooltip(cx, ...)` to show the name on hover.

## Accessibility

Icons are decorative by default and hidden from assistive technology. Naming an
icon with `.accessibility_label(...)` or `.accessibility_labelled_by(...)`
publishes it with the `Image` role. An icon inside a button or icon button stays
hidden; the button carries the name.

## Built-in icons

The built-in set is 56 icons from Lucide 1.61.0 (`lucide-static` on npm), in a
24×24 view box:

- Navigation: `chevron-up`, `chevron-down`, `chevron-left`, `chevron-right`,
  `arrow-up`, `arrow-down`, `arrow-left`, `arrow-right`.
- Actions: `x`, `check`, `plus`, `minus`, `search`, `settings`, `ellipsis`,
  `ellipsis-vertical`, `menu`.
- Files: `file`, `file-text`, `folder`, `folder-open`, `save`, `copy`,
  `clipboard`, `trash`, `pencil`.
- Toggles and media: `eye`, `eye-off`, `lock`, `lock-open`, `refresh-cw`,
  `rotate-ccw`, `rotate-cw`, `play`, `square`, `pause`.
- Status: `info`, `circle-alert`, `triangle-alert`, `circle-check`, `circle-x`.
- Panels: `panel-left`, `panel-bottom`, `columns-2`, `list`, `layout-grid`.
- Other: `funnel`, `text-wrap`, `terminal`, `git-branch`, `bug`,
  `external-link`, `download`, `upload`, `house`, `star`.

The Rust constant is the name in upper snake case, such as `Icon::LOCK_OPEN`.
`lock-open`, `funnel` and `text-wrap` are the current names of Lucide's older
`unlock`, `filter` and `wrap-text`.

Lucide is ISC licensed, and some icons derive from Feather under the MIT
license. Both license texts are in `crates/rxui/src/icons/LICENSE-lucide.txt`,
next to the data, and ship in the crate package.

### Updating the set

`crates/rxui/src/icons/lucide.rs` is generated by
`crates/rxui/tools/lucide_icons.py`, which is not part of the published package.
The script converts `<circle>`, `<rect>` (with rounded corners), `<line>`,
`<polyline>` and `<polygon>` elements into path data and records the Lucide
version in the file header. Treat the downloaded package as untrusted data: unpack
it in its own empty directory and run the script from elsewhere.

```sh
mkdir /tmp/lucide && cd /tmp/lucide
npm pack lucide-static@1.61.0 && tar xzf lucide-static-1.61.0.tgz
python3 -I path/to/rxui/crates/rxui/tools/lucide_icons.py \
    /tmp/lucide/package path/to/rxui/crates/rxui/src/icons/lucide.rs
cp /tmp/lucide/package/LICENSE path/to/rxui/crates/rxui/src/icons/LICENSE-lucide.txt
```

Edit the `ICONS` list in the script to add an icon.

## Built-in controls using icons

The tree disclosure chevrons, the tab close button (an `icon_button`), the select
chevron and the select list's check mark are icons. The check box tick, mixed
dash and spinner stay as tuned crate-private marks.

## Verification

Unit tests cover every path command in both forms, implicit repetition, compact
numbers and exponents, smooth-curve reflection, arcs (sweep, large-arc, scaled-up
radii, rotation, zero radii), malformed data offsets, and view box validation.
Every built-in icon parses with non-empty bounds inside its view box. Headless UI
tests cover equality and shared clones, density-following and explicit sizes,
hidden and named semantics, the missing-name error, and centered icon rows. An
opt-in GPU test checks that an icon button paints its icon in the resting,
hovered and disabled text colors.
