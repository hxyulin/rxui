# Fonts

Status: implemented with `rendering` (UiPainter) and `native` (Application).
Headless `TextMeasure` adapters choose their own metrics and ignore fonts.

## Registering fonts

Fonts belong to the painter that measures and draws text. Nothing is discovered
implicitly by `UiPainter`; `Application` discovers installed fonts once by default.

```rust,no_run
use rxui::prelude::*;

struct App;
impl View for App {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().gap(8.)
            .child(label("Default UI text uses Inter"))
            .child(label("let x = 1;").font_family(FontFamily::Monospace))
            .child(label("Brand title").font_family("Brand").font_size(24.))
    }
}

fn main() -> Result<(), ApplicationError> {
    Application::new()
        .font(include_bytes!("../fonts/Inter-Regular.ttf").as_slice())
        .font(include_bytes!("../fonts/Inter-Bold.ttf").as_slice())
        .font(include_bytes!("../fonts/JetBrainsMono-Regular.ttf").as_slice())
        .system_fonts(true) // keep installed fonts for fallback
        .map_font_family(FontFamily::SansSerif, "Inter")
        .map_font_family(FontFamily::Monospace, "JetBrains Mono")
        .map_font_family("Brand", "Inter")
        .run(|cx| {
            let app = cx.new(|_| App);
            cx.open_window(WindowOptions::new(), app)?;
            Ok(())
        })
}
```

- `Application::font(bytes)` adds TrueType/OpenType data, including collections. The
  first call turns off system discovery; `.system_fonts(true)` turns it back on so
  installed fonts remain available for fallback. Invalid data fails `run` when the
  first window is created.
- `Application::map_font_family(family, target)` resolves a generic family
  (`SansSerif`, `Serif`, `Monospace`) or a named alias to a loaded family name.
- `AppContext::register_font(bytes)` works after startup, for example from a
  settings screen. It validates the data immediately and returns the faces it
  adds, each with its family names, PostScript name, weight, slope and whether it is
  monospaced. The host loads it into the shared painter before the next frame, and
  every window re-measures its text. `AppContext::map_font_family` is the runtime
  counterpart of the builder mapping.

Custom hosts use the painter directly:

```rust,no_run
# fn setup(painter: &mut rxui::UiPainter) -> Result<(), rxui::UiError> {
use rxui::FontFamily;
let faces = painter.register_font(std::fs::read("Inter.ttf").unwrap())?;
assert!(faces[0].families.iter().any(|f| f == "Inter"));
painter.map_family(FontFamily::SansSerif, "Inter");
# Ok(()) }
```

`register_font` retains the bytes without copying them and returns `UiError::Text`
for unusable data, leaving the painter unchanged. Registering a font, changing a
mapping (`map_family` / `unmap_family`) or calling `fonts_mut` advances the
painter's font generation. Each placement re-measures all of its text on its next
`Ui::prepare`, so register fonts at startup or on explicit user action rather than
per frame. A mapping equal to the current one changes nothing.

## Family matching and fallback

Each text leaf has a resolved [`TextStyle`](styling.md): a family, weight, slope and
line height. Shaping chooses fonts per run of text:

1. A mapping from `map_family` replaces the requested family first. Mapping is one
   step: mapping "Brand" to "Inter" and `SansSerif` to "Brand" does not chain.
2. The requested family is matched by name, then by the nearest weight and slope
   within that family. Italic requests on a family without an italic face are
   slanted synthetically; bold requests use the nearest available weight, with no
   synthetic emboldening. Variable fonts with a weight axis follow the requested
   weight.
3. Unmapped generic families use the font database's defaults, which are Arial
   (sans-serif), Times New Roman (serif) and Courier New (monospace). They exist on
   macOS and Windows but often not on Linux, so map generic families when an
   application bundles its fonts.
4. Characters the chosen face cannot draw fall back, for each run, to the platform's
   script-specific families for the run's Unicode script and locale (for example
   Noto or Hiragino families for CJK), then the platform's common fallback list
   (on macOS: the system UI font, Menlo, Apple Color Emoji, Geneva and Arial Unicode
   MS), then every other loaded face in order of weight and slope closeness.
5. If no loaded face covers a character, it is drawn with the primary face's
   missing-glyph (`.notdef`) box. Ordinary text never fails to shape.

Fallback only considers loaded faces. An application that disables system discovery
and bundles a single Latin font shows boxes for scripts and emoji that font lacks;
bundle fallback fonts too, or keep `.system_fonts(true)`. The platform lists come
from the pinned cosmic-text version through rxui-draw, and the locale is `en-US`.

## Limits

- Font loading is synchronous on the UI thread. System discovery can take tens of
  milliseconds on machines with many fonts.
- Fonts cannot be unloaded. `unmap_family` removes a mapping only.
- Variable fonts expose only their weight axis; other axes keep their defaults.
- Emoji and other color glyphs render when a color font is loaded.

## Coverage text placement

The RXUI drawing adapter prepares built-in labels, rich text, placeholders and text
inputs for their final physical origin using `TextRasterOptions::physical_origin`.
The origin includes input horizontal scrolling and, for wrapped paragraph chunks,
each chunk's vertical offset. Font layout, advances, selection and hit testing
remain fractional and unchanged. Outline masks use rxui-draw's quarter-pixel phase
bins; ink can move by at most 1/8 physical pixel per axis relative to the ideal
position. Color/bitmap artwork and MTSDF retain their existing behavior.

Prepared text is reused for color, selection and whole physical-pixel movement.
A fractional physical-origin change explicitly prepares replacement glyph geometry
for ink that can reach the clip, retaining the shaped layout. Offscreen resources
keep their old phase until revealed; wrapped chunks check visibility individually,
including when the clip moves without changing the base phase. Display-density
changes also prepare replacement geometry. Only fractional origin
is part of the placement cache key: a whole-pixel translation does not require
another raster/upload. Single-line input caret scrolling invalidates the retained
preparation pass even when the value, selection's text revision and layout stay
the same. Paragraph chunks compare their own final origins when an earlier
chunk's height changes.

RXUI uses a positive uniform display transform. Cropped opacity layers shift by
whole physical pixels, and animated layer destinations are pixel-aligned, so they
preserve the same raster phase. Call `UiPainter::prepare` after geometry changes
and use the corresponding scale for normal painting/composition.

Custom elements opt in through
`CustomPrepare::text_raster_options(final_logical_origin)`. Include all custom
line offsets, scrolling and text insets in that origin, and retain/reprepare by
fractional physical origin and raster density. Additional custom rotation,
nonuniform scale or transforms require explicit rxui-draw options for that draw;
the default filtered preparation remains available. See
[custom-elements.md](custom-elements.md) for the preparation and paint contract.
