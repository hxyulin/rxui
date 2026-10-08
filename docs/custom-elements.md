# Custom elements

Status: implemented. Measurement and hit testing need the default `layout` feature;
preparation and painting need `rendering`.

A custom element is an application-defined leaf. It takes part in layout like a
label or image, receives pointer input through the ordinary listeners, and paints
with the Astrelis painter. Implement `CustomElement` and wrap the value with
`custom(...)`:

```rust
use rxui::{CustomElement, CustomMeasure, ElementInfo, UiError, custom, prelude::*};
use rxui::astrelis::{PaintSession, Rect};

#[derive(PartialEq)]
struct Meter { fraction: f32 }
impl CustomElement for Meter {
    type State = ();
    fn measure(&self, request: CustomMeasure) -> [f32; 2] {
        [request.known[0].unwrap_or(120.), 8.]
    }
    fn paint(&self, _: &(), element: &ElementInfo<'_>, paint: &mut PaintSession<'_, '_>)
        -> Result<(), UiError> {
        let b = element.content_bounds;
        paint.fill_rect(Rect::new(b.x, b.y, b.width * self.fraction, b.height), element.color)?;
        Ok(())
    }
}

custom(Meter { fraction: 0.4 })
    .key("progress")
    .fill_width()
    .accessibility_role(SemanticRole::Image)
    .accessibility_label("40% complete")
```

## Contract

- **Description.** The value is part of the view's owned description and is rebuilt
  on every evaluation. Reconciliation keeps the retained node while the element type
  stays the same. `PartialEq` decides whether layout must measure it again: an equal
  value keeps its measurement, an unequal one is measured on the next prepare.
  Painting always uses the latest value.
- **Measurement.** `measure` returns the content size in logical units, excluding
  padding and border. `CustomMeasure` carries the sizes layout already fixed
  (`known`), the space the parent offers (`available`, a Taffy `AvailableSpace`)
  and the inherited font size. Explicit sizes from `width`, `height` and friends
  win over the measurement. Non-finite or negative results fail preparation with
  `UiError::InvalidGeometry`. Font size and typography changes measure it again.
- **Hit testing.** `hit_test` receives a point relative to the border box. It
  narrows targeting for elements that already take pointer input (pointer
  listeners, a cursor, `focusable(true)` or `PointerEvents::Block`) and for wheel
  scrolling. The default accepts the whole box.
- **Preparation.** `prepare` runs in `UiPainter::prepare` for every visible custom
  element, outside any render pass. `CustomPrepare` lends the Astrelis `Painter`
  (text, path and image preparation), the `TextSystem` for shaping, the frame's
  render format and the raster scale. Load fonts through `UiPainter::fonts_mut`
  instead, so text measurement is invalidated.
- **State.** `State` is created with `Default` on first preparation and retained
  by the painter per element identity, like prepared text. It is replaced when the
  element at that identity changes to a type with a different state type, and
  dropped when the element is removed or `UiPainter::forget` releases the placement.
- **Painting.** `paint` records draws after the element's shadow, background and
  border, and before focus decoration. The session is already transformed to
  logical units and clipped to the element's clip, including a rounded ancestor
  clip. `ElementInfo` gives bounds, resolved paint (text color, background, radii),
  typography and interaction state. Keep ink inside the element's bounds: culling
  and opacity layers only account for the bounds.
- **Semantics.** Custom elements default to `SemanticRole::Container`. Describe them
  with the ordinary builders: `accessibility_role`, `accessibility_label`, and the
  value and state builders in the [semantics contract](semantics.md).

Custom elements are leaves and reject children. Compose them with containers to
mix them with labels and images.
