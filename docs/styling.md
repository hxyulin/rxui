# Themes and retained styling

Status: implemented with the default layout feature. `rendering` paints the
resolved styles; `native` adds application/window theme selection and live updates.
Themes are placement state, independent of shared entity fields and native surfaces.

## Application and element API

```rust
use rxui::prelude::*;

let theme = Theme::dark()
    .colors(|colors| colors.focus = rgb8(180, 215, 255))
    .metrics(|metrics| metrics.radius = 6.);

// Native host default for windows without an explicit theme:
let app = Application::new().theme(theme);
```

Dark is the default; light uses the same metrics and semantic tokens. Theme clones
share immutable data. Customization creates a new value without changing existing
clones. Theme::palette and Theme::sizes expose the chosen colors/metrics; color
resolves one ThemeColor. Invalid palettes/metrics are rejected when installed.

```rust
column().padding(24.).gap(12.)
    .color(ThemeColor::Text)
    .child(label("Profile"))
    .child(label("Changes are saved locally").color(ThemeColor::TextMuted))
    .child(button("Save")
        .hover_style(PaintStyle::new().border_color(ThemeColor::Focus)))
```

Color builders accept a literal linear RGBA array or a ThemeColor token. A token
retains its meaning and resolves again after a theme switch. `rgb8` and `rgba8`
convert sRGB byte channels to linear RGBA; alpha remains linear. Dividing familiar
hex RGB channels by 255 without conversion would produce the wrong renderer color.

Applications extend a theme with their own tokens. `define_color(name, color)`
adds a color used as `ThemeColor::Custom(name)` anywhere a color is accepted, and
`define_metric(name, value)` adds a number that views read with
`cx.theme().custom_metric(name)`:

```rust
let dark = Theme::dark().define_color("sidebar", rgb8(24, 26, 31)).define_metric("gutter", 12.);
let light = Theme::light().define_color("sidebar", rgb8(236, 238, 241)).define_metric("gutter", 12.);

column().background(ThemeColor::Custom("sidebar"))
```

Custom tokens resolve and switch like built-in ones. Define the same names in every
theme an application switches between: an undefined color resolves to transparent,
and `Theme::custom_color`/`custom_metric` return None for undefined names. Names
are static strings so tokens stay cheap to copy and compare.

`Element::theme(theme)` overrides the complete subtree, including stateful child
components. Sharing an Entity<View> does not share its placements' theme, focus,
selection or scroll offsets. The same component can appear inside dark and light
scopes without reading a global palette during description construction.

## Inheritance and precedence

Text color and font size inherit through ordinary containers and component
boundaries. An inherited color token resolves against each descendant's nearest
theme; an inherited literal stays literal. An explicitly inherited font size
survives a nested theme, while an unspecified size uses the nearest theme default.
Backgrounds, borders, radii, padding and dimensions do not inherit.

For a control, resolution combines its theme defaults and inherited text styling,
then ordinary explicit paint builders, then the active local state patch.
`disabled_style` takes precedence over `pressed_style`, which takes precedence over
`hover_style`. Focus remains a separate ring over whichever state is active. It
surrounds the border box outside it, `ThemeMetrics::focus_offset` (one unit)
away, following the corner radii; list rows draw it inside instead. Its
color, width and offset can be customized with PaintStyle or the theme. Like a
shadow, the ring is clipped by clipping ancestors. A focused text input also
outlines its border in the focus color. The ring is drawn only
when `ElementInfo::focus_visible` is set: after keyboard navigation and on text
inputs, not after a pointer press (see [focus rings](native-feel.md#platform-conventions)
and `FocusRingPolicy::Always`). Inactive native
windows retain logical focus; input caret visibility still follows native activation.

State patches affect local painting only, including local text color; they do not
alter descendants' inherited text styling, font size, dimensions or padding.
The current routed hover/pressed states belong to buttons and text inputs. A
passive container's state patch does not create hit testing or interaction.

PaintStyle is a sparse patch. `paint_style` merges specified fields with earlier
builders, and repeated state builders merge their specified fields. An omitted
field preserves its earlier value. `no_background` explicitly removes a control's
default fill, and PaintStyle::no_border hides border painting while preserving its
layout widths. Radius rounds the painted contour. On an element that also clips
(`clip`, `scroll_x`, `scroll_y` or `scroll`), descendants are painted inside its
content box rounded by the radii inset by its border and padding. Only the nearest
rounded clipping ancestor is applied; outer ancestors still clip rectangularly.
Hit testing always uses the rectangular clip. `ElementInfo::rounded_clip` exposes
the applied clip to custom painters.

```rust
button("Custom")
    .background(rgb8(28, 28, 28))
    .color(rgb8(245, 245, 245))
    .border(1., rgb8(133, 133, 133))
    .radius(8.)
    .paint_style(PaintStyle::new().focus_color(rgb8(245, 245, 245)))
    .hover_style(PaintStyle::new().background(rgb8(48, 48, 48)))
```

Ordinary explicit colors also override theme state defaults. Thus a literal fill
stays that literal on hover unless a hover patch changes it. Focus still appears.
Explicit control builders track their intent: `.padding(0.)`, `.border(0., ...)`,
fixed sizes, fill sizes and `.font_size(...)` survive changes to theme defaults.

`layout` still exposes the Taffy style. It records themed box fields that differ
before/after the closure, preserving those changed values as overrides. To freeze
a value equal to the constructor's existing default, use the dedicated padding,
width, height or border builder; a raw assignment of an identical value is
indistinguishable from leaving that field untouched. Unrelated Taffy changes keep
the themed padding/dimension defaults. `border_top`, `border_right`, `border_bottom`
and `border_left` set one side's width and `border_color` sets the color of every
side. Nonuniform borders are painted as edge strips clipped to the rounded outer
contour, so their inner contour stays square; uniform borders follow the radii on
both contours. `corner_radii(top_left, top_right, bottom_right, bottom_left)` sets
each corner; `radius` sets all four.

`.shadow(BoxShadow::new(color).offset(x, y).blur(b).spread(s))` paints a blurred
shadow of the element's rounded box behind its background. The color can be a
theme token. Shadows are paint-only: they follow the ancestor clip, do not change
layout or hit testing, and keep an element visible to culling while only its shadow
is on screen; a visible focus ring does the same. `elevation(Elevation::Popover)`
binds the theme's shadow for a level instead. `PaintStyle::shadow` and `PaintStyle::no_shadow` change it per state,
for example a stronger shadow on hover.

## Typography

Font family, weight, slope, line height and alignment inherit like font size:

```rust
column()
    .font_family("Inter")              // or FontFamily::Monospace, Serif, SansSerif
    .line_height(1.6)                  // multiple of the font size
    .child(label("Title").font_size(20.).font_weight(FontWeight::SEMIBOLD))
    .child(label("Caption").font_style(FontStyle::Italic))
    .child(label("Total").fill_width().text_align(TextAlign::End))
```

Unset fields inherit from the nearest ancestor that sets them, across component
boundaries. Defaults are the sans-serif family, `FontWeight::NORMAL`, upright text,
the theme's `ThemeMetrics::line_height` and start alignment. A named family must be
loaded through `UiPainter::register_font` (or `Application::font`); otherwise shaping
falls back to another loaded face. [Fonts](fonts.md) covers registration, mapping
generic families to bundled fonts, and the fallback order. To mix weights, slopes,
sizes or colors within one paragraph, use [`rich_text`](rich-text.md) spans. Alignment positions lines inside the text leaf's
content width, so it only moves text in a leaf wider than its text, such as one with
`fill_width` or a fixed width. Single-line text inputs always start-align because
they scroll horizontally from the start edge. Changing any of these attributes
invalidates the leaf's measurement and prepared text; color changes still do not.

`ElementInfo::text_style` and `TextRequest::style` carry the resolved attributes to
custom painters and `TextMeasure` implementations. Rich text spans within one leaf
are not supported yet.

## Live switching and custom hosts

For headless or custom hosting, call `ui.set_theme(theme)?` and prepare before
painting or geometry-based input. It validates before replacing the working theme
and returns false for an identical theme. It marks retained styling pending rather
than marking entity views dirty. Ui::needs_prepare accounts for pending styling;
Ui::is_prepared and snapshots reject a pending theme until preparation completes.
Resolved appearance is exposed through ElementInfo::paint, with actual Taffy
border widths in ElementInfo::border. Custom painters need no ancestor/token walk.

Native applications choose their default with Application::theme. WindowOptions::theme
creates an explicit window override. WindowOptions::background remains an explicit
linear clear color; otherwise the clear follows the effective window theme.

Inside a listener/update, use:

```rust
// All windows following the application default:
cx.set_theme(Theme::light())?;

// One source window, independent of later application switches:
let window = cx.window().unwrap();
cx.set_window_theme(&window, Theme::dark())?;

// Resume live inheritance:
cx.use_application_theme(&window)?;
```

These methods queue native updates outside active entity borrows. Theme selection
is available before queued window creation and survives surface replacement.
WindowHandle::theme reports its latest effective selection; AppContext::theme
reports the application default, which can differ from the source window. Explicit
subtree themes stay independent of both. Invalid themes, foreign-runtime handles
and closed windows are rejected. Setting the theme does not reset editing state,
focus, scrolling, pointer capture or IME sessions. Font/box changes may clamp scroll
offsets to new geometry, as with ordinary reflow.

## Default appearance and accessibility targets

`Theme::dark()` (default), `Theme::light()` and `Theme::high_contrast()` share the
balanced metrics: 13 logical-unit text, 28-unit buttons and inputs (12×3.9 button
padding, 10×3.9 input padding), a 220-unit default input width, one-unit borders,
six-unit control radii (four for small parts, eight for popovers and dialogs) and
two-unit focus rings one unit outside the box (three units in high contrast).
`Theme::compact()` switches any preset to 12-unit text and 24-unit controls while
keeping its focus ring; `ThemeMetrics::compact()` and `ThemeMetrics::balanced()`
are the raw presets. Text lines are `font_size × line_height` tall, with
`ThemeMetrics::line_height` 1.4 in every preset, so the preset paddings make buttons
and inputs the same height. A theme with a different line height should adjust the
button padding to keep that alignment. A caption button given a fixed height or
less padding keeps its caption vertically centered in the space it has. Composed
content (`button(row()...)`) is laid out by flexbox alignment instead; give such a
row `.align_items(AlignItems::CENTER)` so an icon and its label share a center line.
Font loading remains the painter's policy.

`.text_role(TextRole::...)` sizes text from the theme's type scale and follows
theme switches: Caption 11, Body 13, Subheading 15, Heading 18 and Title 24 in the
balanced metrics (11/12/14/16/20 compact), with the heading roles semibold.
`Theme::generate(ThemeSeed::new(scheme, accent))` builds a complete theme from a
scheme and accent, with `.neutral(hue, chroma)` choosing the tint of its neutrals;
it uses the balanced metrics and the preset shadows of its scheme. Its accent
text follows the same rule as the system accent below.

Check boxes and radios are `ThemeMetrics::indicator_size` square (14 balanced,
12 compact) and switches `switch_width` × `switch_height` (26×16 and 22×13). The
switch height also sizes the switch knob (five eighths of it), the slider thumb
(equal to it), the slider rail (three eighths) and progress bars (half, rounded
down): 16/6/8 units balanced and 13/5/6 compact. Check marks, radio dots and
corner radii scale with their box. These parts resolve from the nearest theme,
so a density switch resizes them live, and they stay shorter than a caption
line, so a captioned check box or switch is as tall as a button. Validation
rejects negative or non-finite values, as for the other box metrics.

List rows are denser than controls. An `Element::list_row` without its own height
or minimum height is at least `ThemeMetrics::row_height` tall (24 balanced, 22
compact), and without its own padding has `row_padding_x` on each side (8 and 6)
and no vertical padding; taller content grows the row. A button row keeps its
caption centered in that height. Use `cx.theme().sizes().row_height` as the slot
height of virtual lists and trees so their rows match.

Default buttons use the Control fill, a one-unit Border outline, medium-weight
captions and the theme's `Elevation::Control` shadow (only light has one). Primary
buttons use the accent fill with no visible border, Quiet buttons have muted text
and a RowHover fill while hovered, and Danger buttons have Danger text and border
over no fill, tinted with DangerSubtle while hovered or pressed. Text inputs look
different from buttons: the Input fill (darker than the surface in dark, white in
light), an InputBorder outline that keeps its fill on hover, and a Focus-colored
border while focused.

Popovers, menus, select lists, tooltips and dialogs are `Element::raised`
surfaces: the Raised fill, a one-unit Divider outline, the large radius and the
`Elevation::Popover` shadow, or `Elevation::Modal` for dialogs. Their surfaces
scope focus without a ring of their own; their items show focus. A clipping
element clips only its descendants, so its own shadow and focus ring extend past
its box. Menu items, tree
rows and other `Element::list_row`s paint no box at rest, RowHover while hovered,
pressed or keyboard-focused, and RowSelected when selected, dimming to
RowSelectedInactive while focus is outside their list. Tabs are unboxed: muted
captions, with the selected tab in the text color over a two-unit accent
indicator, on a strip separated from its panels by a Divider line. Scrollbars
have no track fill and a slim, translucent TextMuted thumb that strengthens while
hovered or dragged; split and dock dividers are a one-unit Divider line.

Light shadows match CSS `0 1px 2px`, `0 6px 20px` and `0 16px 40px` in
`rgba(16, 24, 40, a)` at 6%, 18% and 22% for the control, popover and modal
levels; dark uses black at 50% and 60% for popovers (`0 8px 24px`) and modals
(`0 16px 48px`) with flat controls. Blur radii follow CSS (the Gaussian deviation
is half the blur), but RXUI blends in linear light, where a dark translucent
shadow over a light surface looks about half as strong as the same CSS alpha.
`ThemeShadows::light` therefore stores the linear alpha that darkens white as
much as the CSS value does (about 0.13, 0.36 and 0.43). Over dark surfaces the
two blends nearly agree, so the dark alphas are unconverted. Custom
`BoxShadow`s are taken literally.

| Token (sRGB) | Dark | Light | High contrast |
| --- | --- | --- | --- |
| Background | #111214 | #F6F6F7 | #000000 |
| Surface | #17181B | #FFFFFF | #000000 |
| Raised | #1E1F23 | #FFFFFF | #000000 |
| Control | #24262A | #FFFFFF | #000000 |
| Hover | #2C2E33 | #F3F3F5 | #1F1F1F |
| Pressed | #33353B | #EAEBEE | #3A3A3A |
| Main text | #E6E7EA | #1A1B1E | #FFFFFF |
| Secondary text | #9A9DA5 | #646770 | #E6E6E6 |
| Disabled text | #5C5F66 | #A4A6AD | #9A9A9A |
| Border | #34363C | #D3D5DA | #FFFFFF |
| Input / input border | #0D0E10 / #34363C | #FFFFFF / #C9CBD1 | #000000 / #FFFFFF |
| Divider | #26282C | #E4E5E8 | #FFFFFF |
| Accent / hover / pressed | #3A6FE5 / #2F63DB / #335FC8 | #2F6BEB / #245FDD / #1F52C2 | #FFD400 / #FFE34D / #E6BF00 |
| Accent text | #FFFFFF | #FFFFFF | #000000 |
| Focus | #4A7CEF | #2F6BEB | #FFD400 |
| Selection / selected text | #2B4580 / #FFFFFF | #CFDDFB / #1A1B1E | #00E5FF / #000000 |
| Row hover | white at 12/255 | black at 10/255 | #1F1F1F |
| Row selected / inactive | #22314F / #26282D | #E3ECFF / #EDEEF0 | #0050FF / #3A3A3A |
| Danger / warning / success | #E5484D / #F5A524 / #30A46C | #D93036 / #C27C0E / #218358 | #FF4040 / #FF9900 / #00E060 |

Tests verify, for every preset and for generated themes, main text at least 4.5:1
against background, surface, raised, control, hover, pressed and row fills;
secondary text at least 4.5:1 against background and surface; focus at least 3:1
against background, surface, control and input fills; accent text at least 4.5:1
on the accent, its hover and pressed states; selected text at least 4.5:1; and
status colors at least 3:1 against the surface. Button and input borders,
dividers and disabled text are deliberately softer in light and dark, so
check boxes, radios, switches and rails outline themselves with Secondary text
instead; use `Theme::high_contrast()` when every boundary must stand out.
Arbitrary custom colors and inherited/local combinations are not automatically
contrast-corrected.

### Following the system accent

In `ThemeMode::System` with `ThemeSet::follow_accent` (the default), the light or
dark theme takes the operating system's accent; the high-contrast theme never
does. The accent fill keeps the system color's OKLCH hue and as much chroma as the
sRGB gamut allows, and only its lightness moves until the theme's own
`accent_text` (white in the presets) reaches 4.5:1. Hover and pressed step a
further 0.04 and 0.08 of lightness away from the text, darker under white text,
so they stay distinct and readable.

The text flips to black (or to white, for a theme with dark accent text) only
when keeping it would cost the accent more than 0.25 of OKLCH lightness. Beyond
that, light yellows and limes darken into olive and brown and lose their
identity. The approximate macOS accents need these lightness drops for white
text: blue 0.03, purple 0, pink 0.09, red 0.01, orange 0.15, green 0.16 and
graphite 0.08, so all of them keep white text in both schemes and become, for
example, #0072EE for blue, #B85C00 for orange and #2F8700 for green. Yellow
(#FFC600) would need 0.29 and takes black text on its own color, with lighter
hover and pressed states. A white accent also takes black text.

The focus ring starts at the system color and lightens (dark) or darkens (light)
until it has 3:1 against the background, surface, control and input fills.
`selection`, `row_selected` and `row_selected_inactive` keep the theme's OKLCH
lightness and chroma and take the accent's hue. Their chroma is scaled down when
the accent is less colorful than the theme's own, so graphite gives neutral
selections. Each then moves away from its text (`selection_text`, or `text`
for rows) until that reads at 4.5:1. An accent equal to the theme's reproduces
its tokens. Other tokens, including status colors, keep their theme values.

These are design targets informed by [W3C text contrast guidance](https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum),
[enhanced contrast](https://www.w3.org/WAI/WCAG21/Understanding/contrast-enhanced)
and [non-text contrast](https://www.w3.org/WAI/WCAG21/Understanding/non-text-contrast).
They do not establish complete application accessibility. Theme state changes do
not change AccessKit roles/actions. Native applications follow the system
light/dark appearance, high-contrast setting and accent color by default through
`ThemeMode::System`, and match native title bars to explicit themes; see
[native feel](native-feel.md#following-the-system-appearance). Windows forced-color
palettes and theme persistence remain outside this slice.

## Performance and verification

Retained nodes store resolved base/state paint values. Local style changes resolve
the affected subtree, and a root theme switch resolves the retained tree. Palette
changes skip Taffy style replacement, text measurement, layout and text revision
changes. Effective font/box changes invalidate the relevant text/layout inputs.
Hover/pressed/focus choose cached appearance without description/style rebuilding.
Idle preparation skips style traversal when no styling is pending. With a host
that drives frames, state and theme changes interpolate between those cached
appearances over `ThemeMetrics::motion` durations; see [animation](animation.md).

The selected-text foreground uses the existing prepared glyphs with an additional
clipped draw per visual selection rectangle when its color differs from ordinary
text. That adds draw/geometry processing for selections, but no reshaping, new glyph
geometry or uploads. Cost scales with visual fragments and prepared text geometry;
it is not a constant-cost large-document selection implementation.

Headless tests cover token/literal inheritance across component/theme scopes,
explicit zero overrides, palette cache reuse, metric reflow, state precedence,
partial patch merging, invalid values, and selection/composition preservation.
Native command tests cover pending creation, inheritance/reset, invalid themes,
foreign handles and closure. GPU tests assert stable shaped layouts and glyph
uploads across palette changes and validate rounded border/fill pixels in both
presets while restoring caller scissor state.

A separate macOS probe copied the gallery into a one-off binary, added a close
button and opened its windows without activation. Accessibility actions changed
application/window themes, opened an explicit light window and updated shared text.
The inheriting window became dark while the explicit window stayed light; toggling
that window changed its own appearance. Shared text survived each switch, and its
button updated the shared counter. The probe exited through AXPress. Existing user
windows were untouched. The current captures show the unmodified gallery with
system fonts and the macOS blue accent: [dark window](images/theme-dark-validation.png)
and [explicit light window](images/theme-light-validation.png). Each shows the
dark, light and high-contrast presets at balanced and compact density.

```sh
cargo run -p rxui --example theme_gallery --features native --locked
cargo test -p rxui --all-features theme_tests --locked
cargo test -p rxui --features rendering painting::tests --locked -- --ignored
cargo bench -p rxui --bench elements --features accessibility --locked
```

The standalone gallery has no smoke-test flags or shared support module. The
[performance report](performance/themes.md) records timings, sources and limits.

## Composable buttons and variants

`button("Save")` preserves the caption leaf fast path. Composed content uses the
same listener, focus, keyboard and pointer behavior:

```rust
button(row().gap(8.)
    .child(image(self.icon.clone()).width(20.).height(20.).accessibility_hidden(true))
    .child(label("Save")))
    .variant(ButtonVariant::Primary)
    .on_click(cx.listener(|this, _, _| this.save_requested = true))
```

Descendant labels supply the accessible name, including entity-backed components;
explicit accessibility labels override that name. Hidden/decorative subtrees are
excluded. Icon-only buttons need an explicit label. Nested buttons and text inputs
are rejected even through component boundaries. A caption button remains a leaf;
pass a container as its content to add children.

Content text inherits the button's resolved normal/hover/pressed/disabled foreground.
An explicit `.color(...)` on a descendant overrides that inheritance. Image tint is
independent; bind `.tint(ThemeColor::Text)` or provide an application tint when needed.
Pointer/focus state changes select cached paint without layout or new descriptions.
Default is the neutral bordered control, Primary inverts the foreground/background,
Quiet removes resting fill/border while keeping hover/pressed/focus feedback, and
Danger marks destructive actions with danger-colored text and border.
All use the same box metrics, retaining a stable border inset even for Quiet.
`.list_row(selected)` instead paints a button, or any pointer-target element, as a
list row with the denser row metrics.
Explicit paint/state builders override variant defaults, following normal precedence.
Composition adds retained nodes and accessible-name work; the caption fast path
remains useful for large simple lists. See [measurements](performance/images.md).
