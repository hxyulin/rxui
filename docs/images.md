# Images and application GPU output

`Image` is a shared source; `image(source)` describes a placement. Keep handles in
state or an asset cache, then clone them inside views. Creating a new handle for
the same bytes gives it a new identity and requires a new upload. Views perform no
implicit decoding, I/O or GPU work.

```rust
let source = Image::from_rgba8(width, height, rgba)?;
image(source.clone()).width(240.).height(160.)
    .fit(ImageFit::Cover).image_align(0.5, 0.5)
    .accessibility_label("Landscape photograph")
```

RGBA sources require nonzero dimensions and exactly width × height × 4 bytes.
Pixels are straight-alpha sRGB RGBA8. Upload uses an sRGB texture; tints and UI
colors use linear RGBA. `.image_alpha(ImageAlpha::Premultiplied)` selects already
premultiplied RGB for custom sources. GPU framebuffer output defaults to
premultiplied alpha, matching Astrelis blending into a transparent destination.
Shaders that write straight RGB should select Straight explicitly.

The optional `image-decoding` feature adds `Image::decode(&encoded_bytes)` for PNG
and JPEG. It uses ImageReader's default allocation limits, decodes on the calling
thread and converts to RGBA8. It performs no file/network I/O. Load/decode large
assets with `cx.spawn_blocking`, store the resulting handle in the completion,
and let normal model invalidation request drawing. This initial decoder does not
apply EXIF orientation or ICC color-profile conversion. Other formats remain an
application concern; they can supply RGBA pixels directly.

## Placement and layout

One source pixel is one intrinsic logical unit. Explicit sizing is normally
appropriate for high-DPI UI assets. With one auto dimension, measurement preserves
the cropped source aspect ratio; two explicit dimensions avoid layout changes
when a framebuffer changes its backing resolution.

- Contain preserves the aspect ratio and leaves space around the destination.
- Cover preserves aspect and crops the source to fill the content box.
- Stretch fills both dimensions independently.

`.image_align(x, y)` uses normalized 0/start, 0.5/center and 1/end alignment.
`.source_region([u, v, width, height])` restricts the normalized source rectangle
before fitting. Regions must be finite, nonempty and contained in [0, 1].
`.filter(ImageFilter::Nearest)` selects pixel-art/unfilterable sampling instead of
the default Linear. `.tint(...)` accepts literals or theme tokens; it does not
inherit text color. Fit, tint and sampling changes do not remeasure text or run
layout. Crop/aspect changes can reflow auto-sized images.

Images are clipped to the element content box, ancestor clips and the caller's
scissor. A radius rounds the element's painted background/border; texture clipping
currently stays rectangular. Rounded image masks require a later clipping feature.
Name meaningful images with `accessibility_label`; decorative icons should use
`accessibility_hidden(true)`. Image roles do not create pointer or keyboard actions.

## GPU sources

With `rendering`, sources also include:

```rust
let a = Image::from_texture(texture.clone());
let b = Image::from_view(custom_view, [width, height])?;
let c = Image::from_framebuffer(&framebuffer);
// Or retain only its live color source:
let d = Image::from_sampled(framebuffer.sampled_color());
```

GPU sources must belong to UiPainter's graphics device. Textures/views must be
single-sampled 2D color outputs with TEXTURE_BINDING usage and sampling-compatible
formats. Astrelis validates format/usage/sample requirements; wgpu validates raw
texture/view device ownership. Managed framebuffer sources return DeviceMismatch
before constructing their binding. Depth/stencil output and arbitrary storage
textures require an application shader producing a sampleable color image.

Texture/raw-view handles retain their original storage. Pixel writes require a
redraw, without changing identity. A new raw view requires a new Image handle.
Framebuffer handles instead follow the live resolved output across resize and
MSAA changes. MSAA output is resolved before sampling. Configure/resize storage
before UiPainter preparation, then render before the UI pass samples it.
Suspended output draws nothing and keeps its last intrinsic size; recovery refreshes
bindings. A newly suspended source has zero intrinsic size until output exists.
Framebuffer contents must be initialized by a prior submission or earlier pass in
the current frame. Sampling a texture being written in the same pass is rejected.

The application owns rendering policy and the framebuffer rendering owner; RXUI
holds shared sampling handles. Source handles retain their GPU storage until their
last reference disappears. The source and element do not submit work or continuously
redraw by themselves.

## Hosting custom graphics

`Application::prepare_graphics` receives a GraphicsPrepareContext with the source
window, compatible GraphicsContext, WindowMetrics and surface RenderFormat,
plus mutable AppContext. Create/resize targets, upload changed data, prepare
pipelines and publish Image handles here. Updates are flushed and reconciled
before UI GPU preparation. The callback can run during acquisition retries without
a presentation, so avoid unconditional model mutations. There is no implicit
`cx.window()` in this callback; use its explicit source-window handle.

`Application::render_graphics` receives the source window, WindowInfo and the host's
Frame. It records before the UI pass, using the same command encoder and submission:

```rust
let mut pass = frame.render_to(&mut framebuffer)
    .clear_color(astrelis::wgpu::Color::TRANSPARENT).begin()?;
// Draw with application-owned renderers.
```

Drop the pass before the callback returns. The host clears/draws its UI pass and
finishes/presents the frame. Do not finish the host frame, mutate prepared models,
or resize sampled sources during recording. A custom host can perform the same
sequence directly through Ui/UiPainter/Astrelis without either Application hook.
Applications with several windows can key their resources by WindowId and retain
WindowHandles to prune closed-window resources. Hooks receive no automatic
per-window resource container. Request redraw on external GPU writes with
`cx.request_redraw(&window)`; use the runner's continuous policy for animation.

## Caching and evidence

Each UiPainter caches uploaded RGBA storage by ImageId and bindings by source,
filter and alpha encoding. Repeated placements/clones share uploads. Fit, tint,
layout and scrolling do not upload pixels again. GPU sources have no CPU upload.
Framebuffer replacement refreshes bindings without changing source identity.
Removed placements release unused cached resources; `painter.forget(&ui)` releases
one UI while preserving resources referenced by other prepared UIs. Counters from
`painter.image_stats()` report cumulative uploads, bytes and binding creations;
reintroducing an evicted source may upload it again.

Immutable images introduce no idle dimension scan. Live framebuffer placements
are checked for dimension changes; bindings/storage are checked during GPU
preparation. GPU preparation and drawing remain O(retained elements), and each
image placement is currently an individual draw. There is no automatic atlas,
virtualization, encoded-byte content hashing or cross-device cache.

Run the standalone examples:

```sh
cargo run -p rxui --example images_window --features native --locked
cargo run -p rxui --example framebuffer_window --features native --locked
```

The first demonstrates shared RGBA assets, fitting, tint, sampling, composed
buttons and a shared second window. The second owns a 4,096-point offscreen chart,
updates only changed point data, follows window DPI and toggles supported MSAA.
Its preparation and render hooks keep UI code independent of GPU resource mutation.

GPU readback tests check shared-upload counts, alpha blending, caller clipping,
cache ownership across two UIs, resize/MSAA/suspension/recovery, same-submission
rendering, feedback rejection and managed source device mismatch. Native macOS
probes exercise the chart buttons and composable controls through AXPress and
inspect their image/button semantics. See [the measured workloads](performance/images.md)
for CPU preparation/reconciliation costs and their limits.

Native validation captures: [dark image gallery](images/images-dark-validation.png),
[shared light placement](images/images-light-validation.png), and
[offscreen chart](images/framebuffer-validation.png). The shared placements have
active accessibility adapters and show the same changed tint. The host queues
redraws before semantic preparation consumes component dirtiness, preserving
shared-window visual updates while publishing semantics.
