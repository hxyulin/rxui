# Declarative core and explicit hosting

The implemented composition API uses persistent Entity<View> state with owned
element descriptions. A View returns `impl IntoElement`; row, column, label, button and text_input builders return Element. Strings, properties and children belong to the
description. Entity-backed children are erased only at component boundaries.
There is no separately boxed object for every style property.

```rust
use rxui::prelude::*;

struct Counter { count: u32 }
impl View for Counter {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().padding(24.).gap(12.)
            .child(label(format!("Count: {}", self.count)))
            .child(button("Increase").key("increase")
                .on_click(cx.listener(|this, _, _| this.count += 1)))
    }
}
```

## Retained identity and dependencies

`Ui::new(&mut runtime, root)` creates one placement. A child `.child(entity)`
creates a component mount at its retained element position. The child tracks its
own model reads; merely placing it does not subscribe the parent to its state.
Dirty children can evaluate while the parent description remains unchanged.
Multiple placements of one entity retain separate element/focus/capture/scroll state.
The shared entity remains authoritative for application data; UI placement state
does not duplicate that data.

Keys are integer or owned string values, unique among siblings. Compatible keyed
nodes survive reorder. Unkeyed nodes use structural position and compatible kind.
A kind change, entity replacement at a component boundary, or removal disposes
that scope. Removed element IDs are never reused. Removed child mounts release
their listeners even when an external strong handle still retains the child
entity. A component cannot recursively place itself or an ancestor; this is
diagnosed before unbounded tree construction.

## Preparation and resource reuse

A host explicitly sequences:

1. Route input/model completions through synchronous updates, then flush effects.
2. `ui.prepare(&mut runtime, logical_size, &mut measurer)` evaluates dirty views,
   reconciles nodes and computes affected Taffy layout.
3. With the rendering feature, `painter.prepare(&ui, format, raster_scale)` prepares
   pipelines and changed text before acquiring a surface frame. Use that same
   UiPainter as the measurer and initialize its fonts explicitly.
4. Acquire a frame, choose a pass/clipping, call `painter.paint(&ui, &mut pass,
   draw_scale)`, then submit/present through the host.

Ui owns no window, fonts, device or executor. TextMeasure exposes intrinsic and
constrained text sizing, plus a generation for changes to external font inputs.
Headless tests can supply deterministic sizing; UiPainter uses real Astrelis
shaping. Logical layout remains independent of raster density and physical
attachments. The host applies one logical-to-physical transform for painting.

Compatible nodes keep Taffy state. Unchanged properties and callback/color-only
changes retain measurement caches. Dirty text/font changes invalidate their leaf;
constraint changes trigger relevant layout. Prepared text is retained by node
identity, text revision, content width, font generation and raster density.
One UiPainter can prepare multiple placements on the same device without evicting
each other's resources; `forget(&ui)` releases a removed placement. Application
calls it during window removal. Warm preparation skips buffer/layout work for
unchanged text. Paint validates
matching resource metadata before recording, without walking text bytes. Color,
position and hover/focus changes reuse prepared glyph resources.

An idle prepare does not rebuild the description, recollect paint order or run
Taffy. It checks root/component dirty state; the component scan is proportional
to the number of mounted components. Rebuilding a dirty component still constructs
and reconciles its whole description. Changing one field in a component describing
1,000 children is not a constant-time operation. Component boundaries and eventual
virtualization are the tools for reducing that work. There is no allocation-count
or partial-surface-redraw guarantee.

## Input and failure behavior

The initial controls support primary pointer hit testing, same-identity capture
and release, sequential focus and semantic activation. Ui returns whether visual
or application state changed so the host can request redraw on demand. Disabled
or hidden buttons are skipped. Pointer and Enter/Space activation converge on the
same ClickEvent listener. Controlled text input now adds keyboard editing, shaped
selection and native IME; see [the editing contract](text-input.md). General event
propagation/default actions, focus scopes and AccessKit output remain later work.

Descriptions are validated before the evaluating component's dependencies commit.
Duplicate keys, invalid sizes/colors and leaf children produce UiError. Later child
or measurement failures can follow earlier committed components; this is not a
whole-tree rollback transaction. Geometry is unavailable after failure until a
successful retry. A recovered retry also restores partially changed tree
relationships. UiPainter rejects stale/missing resources before recording draws.

Layout uses Taffy flex rows/columns and single-cell stacks. Builders provide
spacing, fixed/percent/min/max sizing, flex distribution, positioning and alignment,
with `.layout(...)` exposing Taffy style customization. Text colors/font sizes
inherit through containers. Sibling z order affects painting/pointer targeting;
Tab and semantics keep description order. See [layout.md](layout.md). Taffy customization alone does not supply text alignment or
UI overflow behavior; use explicit clipping/scroll builders for that behavior.

## Scrolling and clipping

`.clip()` limits descendant input and painting to the container content box.
`.scroll_y()`, `.scroll_x()` and `.scroll()` enable retained offsets on selected
axes. Give the container bounded dimensions. Offsets survive compatible keyed
reorders, clamp when content shrinks, and remain independent across Ui placements.
`Ui::scroll(point, delta)` consumes logical motion from the deepest eligible
container and chains unused motion through ancestors. It updates retained geometry
without rebuilding descriptions, running Taffy or changing text preparation inputs.
Focus traversal reveals the newly focused control through its scroll ancestors.

Hit testing respects viewport and ancestor clips. UiPainter converts effective
logical clips to physical scissor rectangles, intersects the caller's original
scissor and restores that scissor after painting. A GPU readback test checks both
ancestor clipping and caller clipping before and after scrolling.

There are no visible scrollbars, kinetic scrolling or virtualized children yet.
Scrolling refreshes retained geometry across the tree; GPU preparation retains all
laid-out text, including offscreen rows. This first slice avoids reshaping on scroll,
but is not a constant-cost viewport or a large-list solution. Virtualization will
need a separate API and representative benchmarks.

## Native hosting and async work

The [single-file window example](../crates/rxui/examples/counter_window.rs) uses the
implemented Application host, shared model windows, scoped Future tasks and retained
scrolling. [The application contract](application.md) describes lifecycle options
and source-window dispatch. Application supplies default system fonts and a worker
pool, with explicit customization for both. Runtime/Ui/UiPainter remain embeddable;
the [custom-host example](../crates/rxui/examples/counter_custom_host.rs) implements
an astrelis-winit Handler directly. Neither example contains a test-only mode or
support module.

The [task contract](async.md) documents owned background work and weak, synchronous
completion updates. Closing a placement does not dispose a shared entity still
owned elsewhere. Controlled single-line editing, selection/IME and clipboard are now implemented.
Portable semantic output and optional AccessKit translation are implemented; see
[the semantics contract](semantics.md). Native Application hosting manages lazy
publication and assistive actions. Themes and inherited text styling are implemented;
[the styling contract](styling.md) describes retained resolution and live switching.
Multiline/undo editing and virtualization remain next milestones.
