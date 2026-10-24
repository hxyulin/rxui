# Rich text

Status: styled spans and links are implemented with `layout` and drawn with
`rendering`. Inline elements and editable rich text are designed below but not
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
are not published. [Links](#links) are the exception: each is focusable,
activatable and published as a Link node.

### Data flow and invalidation

- The description stores the text once plus `(byte range, SpanStyle)` pairs; spans
  with no attributes are not stored.
- Style resolution turns each span into a `ResolvedSpan`, resolving theme colors
  with the node's theme. Text measurement receives them in `TextRequest::spans`.
  Custom `TextMeasure` adapters may ignore them; sizes and families then come from
  the element style only.
- `UiPainter` passes the spans to rxui-draw's `TextBuffer::set_rich_text`. Span colors
  replace the element color for their glyphs and decorations in the same draw; the
  selection foreground is redrawn with span colors disabled so selected text takes
  one color. Long rich text is chunked like plain text, with spans clipped to each
  chunk.
- A change of range, family, weight, slope or size re-measures and reshapes. A color
  change, including a theme switch for theme-bound colors, reshapes the affected
  paragraphs without layout. Theme switches leave literal-colored spans untouched.
- Validation rejects weights outside `1..=1000`, non-finite or non-positive sizes,
  empty family names and invalid literal colors with `UiError::InvalidStyle`.

## Links

```rust
use rxui::prelude::*;

struct Hover { history: Vec<usize> }
impl View for Hover {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let open = cx.listener(|this, event: &LinkEvent, _| this.history.push(event.index));
        rich_text([
            span("Read the "),
            link("guide", open.clone()),
            span(" or the "),
            span("Vec").monospace().link(open)
                .link_url("https://doc.rust-lang.org/std/vec/struct.Vec.html")
                .link_description("Opens the Vec documentation"),
            span(" docs."),
        ])
    }
}
```

`link(text, listener)` is `span(text).link(listener)`. A link span takes the
theme's accent color and an underline unless the span sets its own; `.color(...)`
and `.underlined(false)` override them like any span attribute. `LinkEvent` carries
the link's `index` among the element's link spans in text order, so one listener
can serve generated links. `.link_url(url)` and `.link_description(text)` are
published to assistive technology only. RXUI never opens a URL: the listener
decides what activation does, for example with `desktop-services` URL opening.

A rich text element with links stays one text leaf. Elements without links keep
the plain label paths; links add the work described under costs below. An empty
link span keeps its index but is never hit, focused or published.

### Pointer

- The element is a pointer target only over its links, unless it already was one
  (selectable, a listener or an explicit cursor). Plain text between links passes
  input through to ancestors like a label.
- Hit testing uses each link's shaped selection rectangles, one per visual line
  fragment, from `TextMeasure::text_range_rects`. `UiPainter` answers from the
  rxui-draw text layout. Rectangles are cached per element during preparation and
  requested again only when its text, spans, content width or font generation
  change (`UiStats::link_measurements`), so pointer motion does no text work.
  Both `Ui::pointer` and `Ui::pointer_with_text` route links. The design proposed
  `text_hit_test`; rectangles are used instead because a caret hit test snaps to
  the nearest position, which would make the empty space after a line's end hit a
  link that ends the line. A custom measurer without `text_range_rects` leaves
  links reachable by keyboard and assistive technology only.
- `Ui::hovered_link()` reports the element and link under the pointer. A hovered
  link shows `Cursor::Pointer`, also over selectable text. Another element's pointer
  capture or a selection drag keeps its own cursor.
- Hover and press only change paint. The hovered link's glyphs and decorations are
  drawn again in `ThemeColor::AccentHover`, and the pressed link's in
  `AccentPressed`, clipped to the link's rectangles, the same way the selection
  foreground is drawn. Links with an explicit color keep it; they still show the
  pointer cursor. The design proposed re-resolving the span color, but that changes
  the text revision on every hover. That would reshape a paragraph and renew the
  label's AccessKit text runs, so a paint-only redraw is used instead.
- A primary press followed by a release over the same link activates it, as buttons
  do. The press also focuses the link; under the default focus ring policy the ring
  stays hidden after pointer input. A press whose pointer-down listener calls
  `prevent_default()`, a release elsewhere, other buttons, cancellation and leaving
  the window do not activate.

### Keyboard focus

- Focus stays an `ElementId`; `Ui::focused_link()` is the focused link's index
  within it. Rich text with links is focusable like a control: `.focusable(false)`
  removes it from focus and `.tab_stop(false)` from Tab traversal.
- Tab moves through the element's non-empty links in text order before leaving it;
  Shift+Tab enters at the last link and walks back. Focus scopes, cycles and
  remembered group focus treat the element as one participant.
- `Ui::focus(id)`, a `FocusHandle` and assistive Focus on the element focus its
  first link. When the description changes, the focused index is kept while that
  link still exists, otherwise focus moves to the first remaining link. Removing
  every link removes the element's focus.
- Enter without Control, Alt or Meta activates the focused link as a `Ui::key`
  default and reports `default_prevented`, so hosts skip their own Enter default.
  Key repeats are swallowed, so holding Enter activates once. Space does nothing
  on a link, as on the web, and `Ui::activate_focused` remains for buttons.
- The focus ring surrounds each line fragment of the focused link, outset by the
  ring width, instead of the element. Moving focus between links reveals the
  link's first rectangle through scroll ancestors.

### Semantics

- The element stays a Label whose value is its plain text. When only its links can
  take focus, the label does not advertise Focus itself. `SemanticNode::links`
  counts its links, and `Ui::semantic_links(id)` returns a `SemanticLink` for each:
  index, name (its text), byte range, URL, description, logical bounds (the union of
  its rectangles), focus and availability.
- AccessKit publishes a `Link` child of the label for each link, after the text
  runs of selectable text. It is named by the link's text and has bounds relative to
  the label, the URL and description, and Focus and Click actions while the element
  accepts input. Accessibility focus is on the focused link's node. Native requests
  decode to `SemanticAction::FocusLink` and `SemanticAction::ActivateLink`, which
  use the same focus and listener paths as Tab and Enter. Link node identities are
  stable while the element lives. Hover is not a semantic change.
- Link nodes have no text run of their own. Assistive technology reads the label's
  value and the links' names; character-level text navigation within a link is not
  published.

### Selection

Selectable rich text keeps selecting across links. A press on a link places the
caret; once the pointer moves more than 4 logical units, the click slop used for
click counting, the gesture selects and the release does not activate. A press
between links focuses the text itself with no link focused; Tab from there enters
the first link. Unlike plain selectable labels, selectable rich text with links
stays in Tab traversal through its links.

### Costs and custom hosts

- Without links: no geometry requests, semantic nodes or per-event work.
- With links: one `text_range_rects` call per link when the element's text, spans,
  width or fonts change, a rectangle test for each rich text element under the
  pointer during targeting, and one semantic node per link.
- Custom measurers implement `TextMeasure::text_range_rects` to make links
  pointer targets. Custom hosts route keys through `Ui::key` and honor
  `default_prevented` before their own Enter handling, and use `Ui::focus_next`
  for Tab, as the native host does.

The [rich text links example](../crates/rxui/examples/rich_text_links.rs) shows a
hover documentation card whose links navigate between symbols and diagnostics
whose locations and error codes are links:

```sh
cargo run -p rxui --example rich_text_links --features native --locked
```

Headless contracts live in `crates/rxui/tests/rich_text_links.rs`: pointer routing,
activation, focus traversal, Enter, selection drags, inert subtrees, semantics and
AccessKit nodes. The test binary is separate from the library's unit tests to keep
the unit-test object within macOS linker limits. The ignored GPU test there shapes
a link with the bundled font and checks the hover recolor and focus ring in pixels:

```sh
cargo test -p rxui --features rendering --test rich_text_links -- --ignored
```

## Design: inline elements

Icons or badges inside a paragraph need rxui-draw support for inline boxes
(reserved advances with a baseline) in `TextBuffer`. RXUI would then measure the
inline child, pass its size as a placeholder span, and position the child from the
reserved box after layout. Until rxui-draw supports inline boxes, compose rows of
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
