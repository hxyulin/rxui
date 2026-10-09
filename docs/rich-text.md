# Rich text

Status: styled spans are implemented with `layout` and drawn with `rendering`.
Links, inline elements and editable rich text are designed below but not
implemented.

## Styled spans

```rust
use rxui::prelude::*;

struct Status { failures: usize }
impl View for Status {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        rich_text([
            span("Build "),
            span(if self.failures == 0 { "passed" } else { "failed" })
                .bold()
                .color(if self.failures == 0 { ThemeColor::Text } else { ThemeColor::Accent }),
            span(" — see "),
            span("target/report.html").monospace().underline(),
        ])
        .fill_width()
        .selectable(true)
    }
}
```

`rich_text(spans)` is a label whose text is the concatenation of its spans. A
`TextSpan` may set a family (`.monospace()` for inline code), weight (`.bold()`),
slope (`.italic()`), font size, color (literal or a `ThemeColor` token), underline
and strikethrough. Unset attributes inherit the element's resolved text style and
color, so a rich text label inside a button or a themed container behaves like a
plain label. Plain strings convert into unstyled spans.

The element is an ordinary text leaf: it measures, wraps, aligns, clips and culls
like `label`, reports `ElementType::Label`, and accepts `.selectable(true)`.
Assistive technology receives its plain text with the Label role; span attributes
are not published.

### Data flow and invalidation

- The description stores the text once plus `(byte range, SpanStyle)` pairs; spans
  with no attributes are not stored.
- Style resolution turns each span into a `ResolvedSpan`, resolving theme colors
  with the node's theme. Text measurement receives them in `TextRequest::spans`.
  Custom `TextMeasure` adapters may ignore them; sizes and families then come from
  the element style only.
- `UiPainter` passes the spans to Astrelis `TextBuffer::set_rich_text`. Span colors
  replace the element color for their glyphs and decorations in the same draw; the
  selection foreground is redrawn with span colors disabled so selected text takes
  one color. Long rich text is chunked like plain text, with spans clipped to each
  chunk.
- A change of range, family, weight, slope or size re-measures and reshapes. A color
  change, including a theme switch for theme-bound colors, reshapes the affected
  paragraphs without layout. Theme switches leave literal-colored spans untouched.
- Validation rejects weights outside `1..=1000`, non-finite or non-positive sizes,
  empty family names and invalid literal colors with `UiError::InvalidStyle`.

## Design: links

Goal: `span("docs").link(listener)` inside `rich_text`, activated by pointer,
keyboard and assistive technology, styled as a link, without making every rich
text element a container.

API:

```rust
rich_text([
    span("Read the "),
    link("guide", cx.listener(|this, _: &LinkEvent, cx| this.open_guide(cx))),
    span(" first."),
])
```

`link(text, listener)` returns a `TextSpan` that defaults to the theme's accent color
and an underline, overridable like any span. `LinkEvent` carries the link's index
within the element, so one listener can serve generated links.

Behavior, in the order it would be built:

1. **Pointer.** The element becomes a pointer target only when it has links. A
   press is hit-tested with the existing `TextMeasure::text_hit_test`; the byte
   offset selects the link span. Release over the same span activates it, as
   buttons do on press-and-release. Hover over a link shows `Cursor::Pointer`;
   hover state is per span and changes paint only (a hover color applied by
   re-resolving that span's color, which reshapes one paragraph).
2. **Keyboard focus.** Each link is a focus stop in tree order within its element.
   Focus state becomes `(ElementId, link index)`; Tab moves through an element's
   links before leaving it, and Enter activates the focused link. The focus ring is
   drawn around the link's selection rectangles. This touches focus traversal
   (`focus.rs`), which today assumes one stop per element, so it is the largest
   change.
3. **Semantics.** Each link publishes an AccessKit `Link` child node with its own
   text run and bounds (from selection rectangles), Click and Focus actions, and
   the URL or description supplied by the application. The parent stays a Label.
4. **Selection.** Selectable rich text keeps selecting across links; a drag that
   starts on a link selects rather than activates once it moves beyond the click
   threshold.

Costs: links add per-element span hit testing on pointer events and per-link
semantic nodes. Non-link rich text keeps today's leaf fast paths.

## Design: inline elements

Icons or badges inside a paragraph need Astrelis support for inline boxes
(reserved advances with a baseline) in `TextBuffer`. RXUI would then measure the
inline child, pass its size as a placeholder span, and position the child from the
reserved box after layout. Until Astrelis supports inline boxes, compose rows of
labels and images; they do not wrap together.

## Design: editable rich text

Rich text in `text_area` needs an attributed value type (text plus span ranges) in
`TextChangeEvent`, span remapping through edits (inserted text inherits the span on
its upstream side, deletions clip spans), formatting commands (toggle bold over the
selection) that propose new spans through the same controlled path, and history
steps that record span changes with text changes. The chunked layout and paragraph
diffing already support it. Clipboard exchange of rich formats (HTML/RTF) is a
separate native-services task. This is deferred until a document model is chosen;
applications needing it today render a preview with `rich_text` beside a plain
`text_area`.
