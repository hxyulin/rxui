# Group-opacity measurements

Recorded on 2026-10-07, Apple M3 Pro, Metal, macOS 27.0.1, Rust 1.98.1,
optimized bench profile. Three runs on an interactive development machine. Raw
CSVs, source fingerprint and reproduction details are in
[compositing/metadata.json](compositing/metadata.json).

```sh
cargo bench -p rxui --bench compositing --features rendering --locked
cargo bench -p rxui --bench compositing --features rendering --locked -- --text-only
```

Ten warmup operations precede ten batches of twenty operations for each case.
Values below are ranges of per-run medians in **microseconds per operation**.
Each sample averages a batch, so these are not individual-frame percentiles.

| Workload | Rectangles | Cached GPU preparation (µs) | Root update + UI/GPU preparation (µs) | CPU encoding + discard (µs) | Encoding + submit/wait (µs) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Direct | 16 | 1.8–2.1 | 9.4–10.4 | 4.3–4.8 | 296.2–817.1 |
| One group | 16 | 1.8–2.1 | 12.1–14.0 | 5.4–6.1 | 343.2–1299.8 |
| Two nested groups | 16 | 1.9–2.2 | 13.4–15.2 | 6.2–7.1 | 377.5–2154.8 |
| One group per rectangle | 16 | 1.9–2.1 | 18.2–18.8 | 19.2–20.8 | 970.0–1045.6 |
| Direct | 256 | 31.8–33.7 | 148.4–154.0 | 61.6–66.0 | 388.2–414.6 |
| One group | 256 | 31.6–33.5 | 190.2–192.8 | 66.2–66.9 | 381.9–426.0 |
| Two nested groups | 256 | 31.8–33.8 | 188.0–198.0 | 63.7–68.9 | 460.0–504.7 |
| One group per rectangle | 256 | 31.8–32.5 | 263.5–267.3 | 286.6–307.4 | 11176.0–11708.9 |
| Direct | 1,000 | 121.3–128.2 | 579.9–605.7 | 237.8–242.5 | 792.0–796.6 |
| One group | 1,000 | 120.8–124.6 | 722.7–737.5 | 245.4–250.0 | 824.8–949.7 |
| Two nested groups | 1,000 | 120.4–123.2 | 731.5–859.0 | 246.5–256.6 | 839.5–1062.0 |
| One group per rectangle | 1,000 | 122.5–138.1 | 1034.9–1068.1 | 1205.2–1300.9 | 41903.2–42467.7 |

## What is timed

The fixture is one 800×600 RGBA8 linear offscreen target at 1× scale, no MSAA.
It paints keyed, opaque 16×16 solid boxes at 20-unit spacing in forty columns.
Counts exclude structural containers. There is no text, image decoding, native
input or surface presentation. Each individual group therefore has a small 256-pixel
output; one combined group uses its cropped union rectangle.

Direct paints all boxes into the destination. One group wraps their common parent
with opacity 0.5. Nested adds a second wrapping group. Many groups applies opacity
to every box independently. The intended visual grouping differs; they are cost
fixtures, not interchangeable semantics.

Cached preparation calls UiPainter::prepare for an unchanged snapshot. Root update
rebuilds/reconciles the whole description, toggles alpha between 0.5/0.6 where
applicable, then prepares the UI/resources/plan. Assertions verify zero additional
layout passes, text measurements and layer allocations after warming. The direct
fixture updates the same alpha field but leaves all element opacity at one, giving
a whole-description rebuild comparison.

Encoding records layers plus the final clear/UI pass and drops the frame. It
includes CPU validation, uploads/driver recording and drop work; it executes no
submitted GPU commands. Submit/wait records, finishes and waits for the exact
submission to complete. It includes that CPU encoding, driver and queue/wait costs;
**it is not a measurement of isolated GPU shader execution**. Separate cases cannot
be subtracted reliably to infer GPU time.

## Interpretation and limits

At 1,000 rectangles, one group uses one extra pass/composite and about 0.72–0.74 ms
for whole-root update/preparation, versus about 0.58–0.61 ms direct. Its encoding is
about 0.25 ms versus 0.24 ms direct. Submitted completion is roughly 0.82–0.95 ms
versus 0.79–0.80 ms direct for this workload. Cached preparation stays near 0.12 ms
with or without the layer. This compares paths in this implementation; it is not a
historical regression benchmark against the previous commit.

One thousand independent groups cost about 1.21–1.30 ms to encode and
41.9–42.5 ms to submit and complete, even though their combined resolved pixel area
is smaller than the one-group union. Many passes/targets carry substantial driver
and GPU overhead. The slowest twenty-frame batch averaged 56.5 ms per operation in
that case, showing additional variability. This path does not meet a 60 Hz frame
budget. Prefer an isolated panel when that is the requested appearance; independent
animations still have independent grouping semantics and costs. Damage/content
caching or grouping optimizations would be separate performance work.

Warm allocations remain 0/1/2/N for direct/one/nested/many groups throughout updates
and recordings. At 1,000 rectangles, one group's resolved output has 394,816 pixels
(~1.58 MB RGBA8), two nested groups have 789,632 pixels (~3.16 MB), and separate
outputs total 256,000 pixels (~1.02 MB), excluding object/driver overhead. Higher DPI,
MSAA, larger clipped unions, HDR storage and text/image workloads change these costs.
Layer contents repaint on every compose; this version caches storage/resources,
not rendered content. General UI responsiveness, GPU timestamps and native vsync
latencies are outside this benchmark.

## Retained text variant

`--text-only` adds one label containing "Rx" at font size six to each of 1,000
rectangles, using the bundled Source Sans 3 test font. Structural count therefore
roughly doubles. Cold font loading/shaping/rasterization/geometry preparation are
excluded. Alpha updates assert no additional layout, measurement, layer allocations
or glyph geometry bytes. Three runs use the same batching and linear target as above.

| Workload | Cached GPU preparation (µs) | Root update + UI/GPU preparation (µs) | CPU encoding + discard (µs) | Encoding + submit/wait (µs) |
| --- | ---: | ---: | ---: | ---: |
| Direct with text | 275.7–281.3 | 1275.3–1321.1 | 580.5–584.7 | 2295.2–2397.8 |
| One group with text | 276.5–283.6 | 1623.7–1650.2 | 601.7–608.1 | 2379.7–2467.2 |

Native verification exposed missing unrelated glyphs at layer-to-direct transitions
when retaining cached pass bindings. RXUI conservatively reapplies text pass state
before retained text drawing, including the ordinary opacity-one path. It still
reuses shaping, atlases and prepared geometry; no glyph uploads occur in painting.
The text results include that rebind cost. This is a correctness workaround for the
observed native Metal behavior, not an established diagnosis of an upstream wgpu
bug. Headless pixel tests alone did not reproduce the missing native glyphs.

## Behavior and native verification

CPU tests check value validation/retry, local rather than inherited opacity,
unchanged geometry/measurement/identity, preserved zero-opacity input/semantics,
caption-child composition and cleanup. GPU readback tests check overlapping and
nested groups, overflowing children, straight-alpha images, glyph ink, live
framebuffer writes in the same submission, 2× DPI, 4× MSAA, layer reuse/replacement,
discard/retry, stale snapshots, sRGB/fractional positioning, negative overflow,
caller viewport/scissor preservation, opacity-to-direct text transitions and
independent placement cleanup. wgpu
validation scopes stay clear.

A one-off binary copied the standalone example with an inactive, always-on-top
window for inspection. Native accessibility actions toggled nesting/opacity and
updated the controlled text field; its edited value remained in the semantic tree
at opacity zero. The unrelated caption and warning regions retain identical bright
pixel counts (1,227 and 10,614) at ordinary/nested 50%, zero and one; the final
native captures were visually checked:

- [Whole-group vs separate cards at 50%](compositing/native-half.png)
- [Nested group at 50%](compositing/native-nested.png)
- [75% with controlled editing via accessibility](compositing/native-edited.png)
- [Zero opacity with unrelated text still visible](compositing/native-zero.png)
- [Opacity one with all unrelated text restored](compositing/native-opaque.png)

These checks verify this macOS/Metal host and normal UI paths; they do not establish
cross-platform behavior or zero-cost compositing.
