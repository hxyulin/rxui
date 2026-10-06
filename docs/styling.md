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
`hover_style`. Focus remains a separate outline over whichever state is active;
its color/width can be customized with PaintStyle or the theme. Inactive native
windows retain logical focus; input caret visibility still follows native activation.

State patches affect local painting only, including local text color; they do not
alter descendants' inherited text styling, font size, dimensions or padding.
The current routed hover/pressed states belong to buttons and text inputs. A
passive container's state patch does not create hit testing or interaction.

PaintStyle is a sparse patch. `paint_style` merges specified fields with earlier
builders, and repeated state builders merge their specified fields. An omitted
field preserves its earlier value. `no_background` explicitly removes a control's
default fill, and PaintStyle::no_border hides border painting while preserving its
layout widths. Radius affects the painted contour, not rounded descendant clipping
or hit testing; use clip/scroll for the existing rectangular clipping behavior.

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
the themed padding/dimension defaults. Nonuniform Taffy borders are laid out and
painted as separate square edge strips; uniform borders support rounded contours.

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

Presets use grayscale surfaces and explicit opaque control state colors. Both use
16 logical-unit text, 12-unit button padding, 10-unit input padding, a 240×44 default
input box, one-unit control borders, four-unit radii and two-unit inside focus
outlines. Text line height and font loading remain the existing text adapter policy;
this slice adds inherited size, not font-family/weight/style inheritance.

| Token (sRGB) | Dark | Light |
| --- | --- | --- |
| Background | #121212 | #FAFAFA |
| Surface | #1B1B1B | #FFFFFF |
| Control | #262626 | #F2F2F2 |
| Hover | #303030 | #E6E6E6 |
| Pressed | #3A3A3A | #D6D6D6 |
| Main text | #F5F5F5 | #181818 |
| Secondary text | #B3B3B3 | #4C4C4C |
| Border | #858585 | #6E6E6E |
| Focus/caret | #F5F5F5 | #181818 |
| Selection | #B3B3B3 | #4C4C4C |
| Selected text | #121212 | #FAFAFA |

Tests verify main text at least 7:1 and secondary text at least 4.5:1 against preset
background/surface/normal/hover/pressed fills; border contrast at least 3:1 against
control fills; focus contrast at least 3:1; selected text at least 7:1; and selection
fill at least 3:1 against the ordinary control fill. Disabled text is also readable
at 4.5:1 against its preset fill. Arbitrary custom colors and inherited/local
combinations are not automatically contrast-corrected.

These are design targets informed by [W3C text contrast guidance](https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum),
[enhanced contrast](https://www.w3.org/WAI/WCAG21/Understanding/contrast-enhanced)
and [non-text contrast](https://www.w3.org/WAI/WCAG21/Understanding/non-text-contrast).
They do not establish complete application accessibility. Theme state changes do
not change AccessKit roles/actions. System appearance following, platform forced
colors, theme persistence and native title-bar appearance are outside this slice.

## Performance and verification

Retained nodes store resolved base/state paint values. Local style changes resolve
the affected subtree, and a root theme switch resolves the retained tree. Palette
changes skip Taffy style replacement, text measurement, layout and text revision
changes. Effective font/box changes invalidate the relevant text/layout inputs.
Hover/pressed/focus choose cached appearance without description/style rebuilding.
Idle preparation skips style traversal when no styling is pending.

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
button updated the shared counter. The probe exited through AXPress. Captures were
visually inspected: [dark window](images/theme-dark-validation.png) and
[explicit light window](images/theme-light-validation.png). The probe used a fixed
Source Sans font and an ASCII value for deterministic capture; the normal gallery
uses Application's system-font discovery. Existing user windows were untouched.

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
and Quiet removes resting fill/border while keeping hover/pressed/focus feedback.
All use the same box metrics, retaining a stable border inset even for Quiet.
Explicit paint/state builders override variant defaults, following normal precedence.
Composition adds retained nodes and accessible-name work; the caption fast path
remains useful for large simple lists. See [measurements](performance/images.md).
