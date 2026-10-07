# Group opacity

`element.opacity(alpha)` applies opacity **once to the entire painted subtree**:
background, border, text, images, descendants, selection, caret and focus ring.
`alpha` must be finite and in `0..=1`; invalid values return
`UiError::InvalidOpacity` during UI preparation. The default is one.

```rust
use rxui::prelude::*;
let panel = column().opacity(0.5)
    .child(label("This whole panel fades"))
    .child(button("Save"));
```

Partial opacity flattens the subtree into a transparent framebuffer, then draws
its premultiplied output once with the chosen alpha. This preserves overlap: two
opaque children overlapping in a 50% group still contribute only 50% coverage.
Putting 50% on each child instead blends them separately. Nested groups first
complete their own layers, then blend into their parent. Their opacity multiplies
along the path, with normal source-over blending between intervening content.
Sibling z order still applies to complete subtrees.

Opacity is paint state. Changing only opacity retains identities, layout and text
measurement. It does not hide semantic nodes or remove pointer/focus targets,
even at zero. Use `inert`, pointer policy or conditional children separately for
those behaviors. Values zero and one are exact fast paths, with no layer storage
or layer pass: zero skips the subtree's painting; one draws directly. Hidden,
fully clipped or visually empty groups also allocate no layer. Text/image
preparation still follows the retained UI snapshot, including zero-opacity nodes.

## Native hosting

`Application` handles composition automatically. Preparation allocates/warmups
resources before frame acquisition. `render_graphics` runs before layer recording,
so live application framebuffer images can be updated in the same frame before
the UI samples them. The final UI pass retains the window's chosen clear color.
Window removal releases its own composition resources.

The standalone [opacity example](../crates/rxui/examples/opacity_window.rs) owns
its application/window, compares whole-group and separate-child fading, toggles
nested opacity and includes controlled text editing:

```sh
cargo run -p rxui --example opacity_window --features native --locked
```

## Custom hosts

The host owns the target, frame, all destination passes, clears/loads, viewport,
scissor and finish/presentation. After `Ui::prepare` and `UiPainter::prepare`, use
`UiPainter::compose` before opening the destination pass:

```rust,ignore
ui.prepare(&mut runtime, logical_size, &mut painter)?;
painter.prepare(&ui, &target.render_format(), scale)?;
let mut frame = target.begin_frame()?;
// Record application textures/framebuffers before this call if the UI samples them.
painter.compose(&ui, &mut frame, scale, |frame, ui| {
    let mut pass = frame.render_pass().clear_color(wgpu::Color::BLACK).begin()?;
    ui.paint(&mut pass)
})?;
frame.finish()?;
```

The callback receives a borrowed `ComposedUi` that cannot outlive the scope. **Use
passes from the frame lent to this callback.** RXUI does not submit that frame and
does not expose a reusable composition token whose contents could survive an
unsubmitted recording. The type system enforces the capability's lifetime, but
cannot verify which frame a destination pass came from. Keep that same-frame
contract when using custom targets or another frame in the callback. Compatible
passes in this recording may paint the composed UI multiple times; normal blending
applies to every such paint.

Prepare for the *actual destination pass* `RenderFormat`, including its color,
depth/stencil format and sample count. That pass can target a caller-selected
framebuffer using `frame.render_to(...)`; it need not be the frame's default target.
With composition, an incompatible destination format returns `InvalidGeometry`
before destination UI draws. Use the same scale for preparation and composition.
A stale plan/resource snapshot is rejected before any layer pass is recorded.
If another error occurs while recording, discard the frame. Every subsequent
`compose` records all layers again; discarded frames leave no stale content cache.

`ComposedUi::paint` preserves the caller's viewport and restores its scissor on
success or error. Final-pass clipping remains authoritative. Layers use their own
full viewport, transparent clear and inherited UI rectangular clips. Existing
`UiPainter::paint` works for ordinary opacity-one snapshots; it returns
`CompositionRequired` if the retained UI contains opacity below one (conservatively
including hidden nodes). Calling `compose` works for all snapshots, without extra
passes for ordinary content. Hosts implementing their own painter can use
`ElementInfo::parent`, local `opacity` and the paint-order snapshot to isolate
subtrees themselves.

## Storage and cost

Layers are cropped to the union of the subtree's visible paint bounds, including
overflowing descendants and actual prepared glyph ink. Control boxes reserve space
for hover, focus and editing decorations. Ancestor/viewport clips bound storage;
DPI rounding expands bounds outward to whole physical pixels. A structural
container's empty box does not by itself expand the layer to the entire viewport.
Very distant visible descendants can still make a large union rectangle.

Targets/bindings are cached per retained element identity and reused across content
or opacity updates with the same size, format and MSAA configuration. Bounds,
DPI, format or sample-count changes replace storage when necessary. Inactive layers
are pruned during preparation. Call `painter.forget(&ui)` when removing a custom
placement; caches for other placements remain available.

Offscreen color has an alpha channel: sRGB destinations use RGBA8 sRGB; linear
RGBA/BGRA8 destinations use RGBA8; 32-bit float destinations use RGBA32 float;
other destinations use RGBA16 float. Alpha is linear and output is premultiplied.
RGBA8 adds intermediate 8-bit quantization; deeper nesting can accumulate rounding.
Layers use the destination's MSAA count and resolve before parent sampling. They
use nearest sampling for pixel-aligned composition, avoiding extra float-filtering
feature requirements. Unsupported layer formats/sample counts or oversized
allocations return Astrelis errors; there is no silent quality fallback. Layers
have no depth/stencil attachment of their own.

Each visible partial-opacity group adds one pass and one composite draw. GPU cost
includes repainting its content plus reading/writing its pixel area. At 2× DPI,
area/storage grows roughly fourfold. An RGBA8 resolved output uses about four bytes
per pixel; multisample storage is additional. `LayerStats` reports cumulative
allocations/recorded passes/composites and current resolved layer counts/pixels;
it reports recordings, not necessarily submitted work or exact GPU memory bytes.

This implementation reuses **storage, pipelines and text/image resources**, but
repaints layer contents every composition. It deliberately does not cache content:
external framebuffer writes, cursor/selection/blink changes and discarded frames
must remain correct without a damage protocol. Prefer fading one panel/subtree over
thousands of independent groups when the intended appearance permits it. See the
[measured costs](performance/compositing.md). Rounded descendant masks, arbitrary
subtree transforms, blur/backdrop filters and general blend modes remain future work.
