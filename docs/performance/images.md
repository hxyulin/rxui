# Image and composed-control preparation measurements

Recorded on 2026-10-07, Apple M3 Pro/macOS 27.0.1, Rust 1.98.1, optimized bench
profile. Three runs enable rendering; CPU text measurement remains a deterministic
mock (8 units per UTF-8 byte, 20-unit height). Exact source fingerprints and raw
results are in [images/metadata.json](images/metadata.json) and its sibling CSVs.
The development machine was interactive; these are not isolated latency results.

```sh
cargo bench -p rxui --bench images --features rendering --locked -- --gpu
```

Each case warms 20 operations and times 20 batches of 50. Values are microseconds
per operation averaged within batches. Ranges show median batch averages across
three runs; p95 is the largest run's p95 batch average. Tiny idle timings are
sensitive to timer amortization and compiler optimization.

| Workload | Placements | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: | ---: |
| Images: idle UI prepare | 16 | 0.041–0.043 | 0.049 |
| Images: unchanged description | 16 | 8.288–8.440 | 12.953 |
| Images: tint change | 16 | 11.245–11.854 | 25.177 |
| Caption buttons: idle UI prepare | 16 | 0.042–0.117 | 0.118 |
| Caption buttons: unchanged description | 16 | 8.132–13.942 | 18.462 |
| Composed buttons: idle UI prepare | 16 | 0.043–0.112 | 0.123 |
| Composed buttons: unchanged description | 16 | 31.586–68.218 | 74.886 |
| Images: cached GPU preparation | 16 | 2.875–2.943 | 2.990 |
| Images: idle UI prepare | 256 | 0.043–0.043 | 0.053 |
| Images: unchanged description | 256 | 122.306–211.695 | 256.118 |
| Images: tint change | 256 | 171.837–340.815 | 359.588 |
| Caption buttons: idle UI prepare | 256 | 0.043–0.044 | 0.454 |
| Caption buttons: unchanged description | 256 | 116.885–233.590 | 291.207 |
| Composed buttons: idle UI prepare | 256 | 0.042–0.043 | 0.050 |
| Composed buttons: unchanged description | 256 | 478.089–743.235 | 1097.264 |
| Images: cached GPU preparation | 256 | 42.060–43.415 | 43.939 |
| Images: idle UI prepare | 1,000 | 0.043–0.053 | 0.074 |
| Images: unchanged description | 1,000 | 490.081–491.696 | 514.348 |
| Images: tint change | 1,000 | 686.345–1032.100 | 1264.808 |
| Caption buttons: idle UI prepare | 1,000 | 0.043–0.043 | 0.052 |
| Caption buttons: unchanged description | 1,000 | 464.741–582.424 | 802.823 |
| Composed buttons: idle UI prepare | 1,000 | 0.041–0.043 | 0.048 |
| Composed buttons: unchanged description | 1,000 | 2011.137–2464.198 | 3926.689 |
| Images: cached GPU preparation | 1,000 | 168.614–174.531 | 184.680 |

## Meaning of the workload

Images share one 32×16 RGBA asset and have explicit 20×20 logical boxes. The
column retains all placements even when offscreen. A caption button is one node;
a composed button contains a row, image and label, giving four nodes per button.
All items have stable keys. Unchanged-description cases invalidate/rebuild the
whole root collection and perform reconciliation, including accessible-name
collection for composed buttons. Tint change alternates the image's token/literal
tint on the entire collection and resolves paint. Counters verify no new text
measurements or Taffy layouts for these cases; no text shaping is timed.

Idle preparation changes no counters and does not scan immutable image sources.
Live framebuffer dimension checks are outside this immutable-source benchmark.
Rebuilding 1,000 composed buttons is a whole-root/four-node-per-item workload,
not the cost of clicking one retained button or updating an isolated child component.
Component boundaries and virtualization matter for large collections. Button
composition deliberately retains the caption leaf fast path.

GPU resource preparation is UiPainter::prepare over an already prepared image-only
UI. It includes retained snapshot traversal, source/view and binding-cache lookup,
placement bookkeeping, pruning and warmed Astrelis pipeline preparation. It creates
no passes, draws, submissions or new pixels. Every count asserts one 2,048-byte upload
and one binding initially, then no changes to those counters throughout timing.
This verifies shared-source reuse but does not time cold allocation/upload or GPU
completion. Distinct assets cost additional storage/bindings; each image currently
issues an individual draw. CPUImage creation/decoding and native hosting are not
included. Allocation counts, retained memory, draw-call costs and GPU bandwidth
remain unmeasured.

## Behavioral verification

Separate native-GPU readback checks validate straight-alpha RGBA over an opaque
background, premultiplied framebuffer output, caller scissor restoration, shared
cache ownership across two UIs, resize/MSAA/suspension/recovery, feedback rejection,
managed source device mismatch and rendering/sample consumption in one submission.
All six ignored GPU tests, including prior text/theme tests, pass on this machine.

Standalone native macOS probes exercised the image gallery and chart via AXPress,
verified image roles, composed-button names/disabled state and changed model labels,
and inspected window captures. Both active shared placements repaint after a
model change; redraw requests are queued before semantic preparation consumes
component dirtiness. They use one-off copies outside the repository, with
no smoke-test branches in the examples. These checks do not provide Windows/Linux
runtime coverage or end-to-end latency. The [image contract](../images.md) describes
ownership, scheduling and current clipping/format boundaries.
