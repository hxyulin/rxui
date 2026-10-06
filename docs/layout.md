# Layout, stacked content and paint ordering

RXUI uses Taffy for flex and single-cell grid layout. Rows/columns remain flex
containers; `stack()` overlaps its children in one grid cell. Grid support is
part of the layout feature, so stacks do not introduce a separate measurement
solver or extra wrapper element for each child. Stateful component wrappers occupy
the same cell as other children.

## Sizing and flex distribution

```rust
row().fill_width().fill_height().gap(16.)
    .child(column().width(180.).fill_height())
    .child(column().flex_grow(1.).flex_basis(0.).min_width(0.).fill_height())
```

The sidebar keeps its width; the second pane receives the remaining main-axis
space. `fill_width`/`fill_height` mean 100% of the containing block, not "take the
remainder after siblings". Use flex distribution for that remainder.

`size(width, height)` is fixed logical sizing. `width_percent`/`height_percent`
accept fractions (1.0 is 100%), including fractions greater than 1 for intentional
overflow. Min/max width/height builders constrain ordinary or flex sizing.
`flex_grow`, `flex_shrink` and `flex_basis` forward Taffy's factors and initial size.
RXUI defaults to zero grow and zero shrink; opt into shrinking explicitly. A flex
item's intrinsic minimum can still limit shrinkage; set `min_width(0.)` or
`min_height(0.)` when appropriate.

`padding_x`/`padding_y` replace one axis without changing the other. On controls,
using a padding builder makes the resulting padding explicit rather than following
later theme metric changes. `margin`, `margin_x` and `margin_y` set external spacing;
negative margins are supported. All numeric sizes/factors must be finite and sizes,
padding and flex factors must be nonnegative. Insets/margins may be negative.

`align_items`/`justify_content` retain their flex meanings. `align_self` overrides
cross-axis alignment on a child. `.layout(...)` exposes the full Taffy Style,
including flex wrapping, aspect ratios, percentages, per-edge spacing and grid
configuration. A row/column can select Display::Grid through that escape hatch;
`stack` itself manages its direct in-flow children's cell placement.

## Stacked and positioned content

```rust
stack().size(400., 240.)
    .child(column().fill_width().fill_height().background(ThemeColor::Surface))
    .child(button("Close").absolute().top(8.).right(8.).z_index(1))
```

In-flow children occupy the same cell; their maximum intrinsic extents determine
natural size, subject to parent constraints, padding and alignment. Absolute
children do not contribute intrinsic size. Default stack child alignment is start
on both axes. `align_items` controls vertical placement and `justify_items`
controls horizontal placement; `align_self`/`justify_self` override those axes for
one child. Stretch alignment remains available explicitly. Ordinary parent sizing
rules still apply, including percentage sizes and flex distribution.

`.absolute()` removes an element from flow. `left`, `right`, `top` and `bottom`
anchor it in its Taffy containing block. `.inset(value)` sets all four edges;
auto-sized children stretch between opposite edges. Explicit dimensions retain
Taffy's over-constrained sizing behavior. `.relative()` restores flow participation;
insets offset the visible placement while preserving its flow slot. These are
layout operations, not paint-only transforms: changes can run layout and reflow text.
Absolute placement works inside rows/columns as well as stacks.

A flex column normally stretches its children horizontally. For natural stack
width inside that column, use `stack().align_self(AlignSelf::START)`. Center
its label with `align_self(CENTER)`/`justify_self(CENTER)` on the label. Otherwise
the label centers over the stretched cell, while a fixed-width background child
can remain aligned at the start of that larger cell.

## Three orders, each with a purpose

The retained child list and semantic children remain in description order.
Tab/Shift-Tab follow that order, preserving compatible keyed identities and focus.
`Ui::elements()` instead iterates paint order. Sibling `z_index(i32)` values sort
ascending; equal values keep description order, so later siblings appear above
earlier ones. Each complete child subtree paints atomically relative to its
siblings. A grandchild cannot use a high z value to escape its parent's position
among sibling subtrees. The parent background/border paints before its children;
negative z values order siblings and do not place children behind that background.

Changing z rebuilds retained paint order but does not alter Taffy geometry, run
text measurement, resolve styling or invalidate prepared glyph resources. Z order
has no automatic effect on keyboard or accessibility navigation. When all z values
are zero, paint order reuses the description-order storage. When z is active, a
separate vector stores paint order; sorting occurs on ordering changes, not idle
preparation. No render target or additional pass is allocated for stacking/z order.

## Overlay pointer and focus policy

Pointer targeting uses reverse paint order, clipped to the viewport and ancestor
clips. Controls receive pointer input by default. Disabled controls still cover
lower controls rather than forwarding clicks through them. Passive content passes
clicks through unless explicitly blocking:

- `PointerEvents::Auto`: ordinary controls receive input; passive content is transparent.
- `PointerEvents::None`: ignore the complete subtree for pointer and wheel targeting.
- `PointerEvents::Block`: this clipped border box blocks lower content; its children
  can still receive input. Hit areas are rectangular, independent of texture alpha.

These policies do not disable keyboard/assistive navigation. For an overlay that
owns interaction, mark its background subtree `.inert(true)` and use a blocking
foreground/scrim:

```rust
stack().fill_width().fill_height()
    .child(background.inert(show_overlay))
    .child(overlay.absolute().inset(0.).z_index(100)
        .pointer_events(PointerEvents::Block))
```

An inert subtree keeps painting, geometry, state and identity but receives no
pointer, keyboard or assistive input and is excluded from semantics. Retained
focus/capture is cleared; active preedit is cancelled before layout so committed
text and geometry remain coherent. Inertness belongs to the placement and
propagates through entity-backed components, without disabling a separately placed
instance of the same entity. Restoring the subtree does not automatically restore
focus. Use conditional children to show/hide overlays; this is a foundation for
modal controls, not an automatic dialog lifecycle, focus-return or portal system.
Wheel targeting follows paint order and pointer policy; motion may still bubble
through scroll ancestors of the selected subtree.

## Compositing boundary and next step

Current compositing covers ordering and ordinary source-over drawing into the
caller-selected pass. Stack, z order and inertness add no offscreen storage. Images
and application framebuffers retain their existing explicit preparation/rendering
contract. Rounded backgrounds do not create rounded descendant masks.

The chosen next rendering step is true group opacity through isolated offscreen
layers. Applying opacity independently to each draw gives incorrect overlap for
fading a subtree, so it will require subtree bounds, reusable layer storage and a
recording stage before painting the final UI pass. The implementation must preserve
caller-owned Frame/pass control, repaint invalidation, alpha encoding and existing
text/image resource reuse. Group opacity, rounded masks and general transforms
are not part of the current layout API.

## Examples and verification

```sh
cargo run -p rxui --example layout_window --features native --locked
```

[The standalone example](../crates/rxui/examples/layout_window.rs) includes a fixed
sidebar, a flexible/scrollable body, natural stacks, anchored cards, z changes and
a blocking overlay with an inert background. It owns its application/window code.

Behavior tests cover sizing, min/max/percent/flex helpers, natural sizing across
component boundaries, alignment, positioning, stable identity/geometry across z
changes, separate focus/semantic order, clipped blockers, pointer transparency,
disabled-control coverage, inertness, preedit cancellation and invalid values/retry.
A GPU readback test verifies scoped paint ordering and ancestor/caller clipping.
The [performance report](performance/layout.md) records workload boundaries and
CPU measurements. Native validation captures are linked there.
