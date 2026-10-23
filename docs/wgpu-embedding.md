# Embedding RXUI in a wgpu application

Use `rxui` with `default-features = false, features = ["rendering"]`. No native
host, windowing crate, surface acquisition or executor is required by that feature.
RXUI reexports its exact wgpu version as `rxui::wgpu`; application resources must
use compatible wgpu types and belong to the same device.

The [standalone winit/wgpu example](../crates/rxui/examples/wgpu_app.rs) starts
with an ordinary application-owned device, queue and surface. It renders an
application texture, displays that texture in RXUI, routes pointer input and
submits application and UI work together. It also replaces the texture on resize
and exercises group opacity. It uses no RXUI native host or managed graphics context.
Its minimal input adapter covers pointer activation; applications needing keyboard,
IME and accessibility must wire those portable UI APIs or use the optional host.

## Preparation and recording

1. Create `UiPainter::new(&device, &queue)`. This shares the supplied handles and
   leaves device error and loss callbacks untouched. Register application fonts.
2. Route input and model changes, then flush runtime effects.
3. Run `ui.prepare` with logical dimensions and the painter as text measurer.
4. Run `painter.prepare` with `UiRenderTarget::render_format()` and raster scale.
5. Record application passes which produce images sampled by the UI.
6. Call `painter.record(&ui, &mut encoder, &target, scale)`.
7. Submit the encoder, call `resources.after_submit(&queue)`, then present if needed.

The target contains the raw color view, its format, physical mip dimensions, load
operation and optional MSAA resolve view. `UiRenderTarget::new` defaults to `Load`
so application content remains visible behind the UI. Choose `Clear` explicitly
when RXUI is the first writer. Sample count comes from the render view. The UI
records its own color pass without a depth attachment; application depth storage
is unaffected. Opacity layers use the same encoder and complete before sampling.

RXUI does not submit, acquire or present in this path. Device-only construction
validates against wgpu's guaranteed format capabilities for the enabled device
features; adapter-specific format extensions are not inferred without an adapter.

## Recorded resource ownership

`record` returns `RecordingResources`. Each recording owns independent upload
storage and retains text/geometry leases. Keep it alive until the encoder is
submitted. Immediately after submission, call `after_submit` on that queue;
allocations then remain pinned until queued work completes.

For abandoned work, discard the encoder before dropping its resources. On a
recording error, discard that encoder. Do not keep unsubmitted commands after
releasing their resources. Multiple outstanding recordings are supported when
these lifetimes are respected. The application controls ordering of external
texture writes and submission; writing the same source texture before an earlier
recording submits naturally changes what that recording will sample.

## Application textures

`Image::from_texture(&texture)` imports a raw texture's default view.
`Image::from_view(view, size)` imports a selected view with explicit intrinsic
pixel dimensions. Use single-sampled 2D sampleable color views. The renderer checks
sampling compatibility; raw view ownership and view descriptors use wgpu validation.
Pixel writes to unchanged storage require redraw, not new image identity.

Use `ExternalTexture::new(view, size)` and
`Image::from_external_texture(source.clone(), ImageAlpha::Premultiplied)` for a
resizable application render target. Choose `Straight` for straight-alpha pixels.
Call `source.replace(new_view, new_size)` between UI preparation/recording steps;
all images sharing the handle retain identity and follow the replacement. Call
`source.suspend()` to paint nothing while storage is unavailable. The application
must schedule redraw and run preparation after replacement.

## Custom renderers

Custom elements retain their existing measure, hit-test, preparation and paint
hooks. `CustomPrepare` exposes the shared raw `device` and `queue`, target format,
scale, and RXUI drawing/text helpers. The format describes the actual attachment,
including RGBA opacity layers for BGRA destinations. Cache custom pipelines by
format and sample count. Prepare application pipelines and buffers
there. The element's `State` owns its retained resources.

Inside paint, `paint.wgpu()` flushes pending UI draws and lends the actual
`wgpu::RenderPass` for your pipelines, bind groups and draw calls. Subsequent UI
drawing restores tracked render state. Raw shaders must handle their own coordinate
transform and rounded clipping; the active rectangular scissor is already set.
For ordinary UI primitives, prefer the RXUI drawing context, which applies the
logical transform and rounded clips. Custom ink must remain within the element's
bounds because culling and opacity-layer bounds rely on that contract.

For renderers requiring compute or separate passes, render into an application-owned
texture before `record` and display it through an image. This supports arbitrary
application renderers without forcing them into RXUI's drawing helpers.

## Optional managed host

`native` supplies RXUI's desktop `Application`. Managed drawing conveniences remain
available under `rxui::drawing`, and lifecycle helpers under `rxui::host`. Their
frames, contexts and framebuffer types are optional conveniences, not prerequisites
for embedding. The external Astrelis renderer and window runner are no longer
dependencies; only the optional experimental native-drag service remains separately
pinned to that repository.
