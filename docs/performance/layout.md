# Layout and paint-order CPU measurements

Recorded on 2026-10-07, Apple M3 Pro/macOS 27.0.1, Rust 1.98.1, optimized bench
profile. Three runs per fixture on an interactive development machine. Raw CSVs,
source fingerprints and reproduction information are in
[layout/metadata.json](layout/metadata.json).

```sh
cargo bench -p rxui --bench layout --locked
cargo bench -p rxui --bench images --features rendering --locked -- --gpu
```

Twenty warmup operations precede 20 batches of 50. Values are microseconds per
operation averaged within each batch. Median ranges span the three runs; p95 is
the largest run's p95 batch average. These are not individual-event latency
percentiles. Small idle values are sensitive to timer amortization and optimization.

| Workload | In-flow stack children | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: | ---: |
| Idle UI preparation | 16 | 0.037–0.043 | 0.047 |
| Unchanged stack description | 16 | 9.050–9.860 | 11.336 |
| Z change + paint snapshot traversal | 16 | 11.312–11.943 | 12.309 |
| Absolute inset change + layout | 16 | 16.204–16.925 | 20.637 |
| Idle UI preparation | 256 | 0.040–0.044 | 0.047 |
| Unchanged stack description | 256 | 113.672–128.456 | 179.440 |
| Z change + paint snapshot traversal | 256 | 144.808–167.190 | 191.546 |
| Absolute inset change + layout | 256 | 198.764–207.459 | 231.113 |
| Idle UI preparation | 1,000 | 0.042–0.044 | 0.047 |
| Unchanged stack description | 1,000 | 444.588–462.633 | 471.819 |
| Z change + paint snapshot traversal | 1,000 | 568.769–587.363 | 635.890 |
| Absolute inset change + layout | 1,000 | 770.649–885.222 | 1100.267 |

## Workload boundaries

The fixture is a fixed 800×600 flex row with a 160-unit sidebar, a flexible
single-cell stack, keyed in-flow boxes and one absolute blocking overlay. Box
sizes vary deterministically; there is no text, font discovery, GPU work or native
handling in this fixture. The count excludes the sidebar, root/container and
absolute overlay nodes.

Unchanged description invalidates and rebuilds the whole root collection and
reconciles it. Z change toggles the first child's elevation relative to the overlay,
rebuilds scoped paint order and iterates the resulting ElementInfo snapshot.
Counters assert no Taffy layout, style resolution or text measurement for z changes.
Absolute inset change moves the overlay between two positions and runs dirty layout
plus retained bounds propagation; all identities remain allocated and no text is
measured. These are whole-root collection updates, not isolated component updates.

Stable default paint order reuses description-order storage. A nonzero z value
requires a separate paint-order vector; child sorting occurs only when ordering
changes. Inertness and pointer policies are inherited/cached during subtree
resolution. None of these mechanisms creates a framebuffer or another render pass.
They do not provide partial-surface redraw, virtualization or damage caching.

## Existing image/content fixture with grid enabled

The same image/content benchmark as [the previous report](images.md) was rerun
with the expanded layout feature. Its GPU phase times warmed resource preparation,
not GPU draws or completion. CPU phases use the same deterministic mock text metrics.
Representative 1,000-placement results:

| Workload | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: |
| Idle images | 0.042–0.043 | 0.048 |
| Unchanged image descriptions | 549.958–557.993 | 568.446 |
| Unchanged caption buttons | 575.920–585.409 | 628.872 |
| Unchanged composed buttons | 2323.053–2376.359 | 2673.089 |
| Cached image GPU preparation | 171.604–175.160 | 181.271 |

Every image count still asserts one 2,048-byte upload and one source binding,
followed by no new uploads/bindings during warm preparation. This is useful
regression evidence, but the earlier runs were taken at a different time on an
interactive machine. Their differences cannot isolate the cost of grid support,
new properties or machine load. Retained component boundaries remain important
for avoiding whole-root rebuilds.

## Memory tradeoff

On this target, `size_of::<Element>()` changes from 576 to 896 bytes and
`size_of::<taffy::Style>()` from 240 to 552 bytes. The baseline is a default-layout
build of commit 7887a81; the current default-layout benchmark prints the new sizes.
Taffy grid adds inline grid configuration/placement fields, and RXUI adds ordering
and interaction properties. Empty grid vectors do not allocate their backing
storage, but their handles still occupy inline bytes. These sizes do not include
heap-owned descriptions, text, textures, retained Node data, Taffy caches or allocator
overhead. They are not total per-widget memory figures.

This is a real memory increase for ordinary flex elements as well as stacks. It
buys one solver for natural overlapping layout and exposes Taffy grid through
`.layout(...)`. Compact internal layout/property storage is a future optimization
worth measuring before large-scale retained UI workloads; these checks do not
claim that the layout expansion is free.

## Behavioral verification

Behavior tests cover natural size across component boundaries, min/max/percentage
and flex sizing, two-dimensional alignment, edge anchoring/stretching, stable keyed
identity, z changes without layout, scoped subtrees, separate Tab/semantic order,
pointer blockers/transparency, disabled-control coverage, inertness across shared
placements and preedit cancellation before layout. A regression test specifically
checks a natural-width stack in a stretching flex column: `align_self(START)` on
the stack keeps the caption centered over its fixed background child.

Seven native-GPU tests pass, including readback of scoped stack ordering and
ancestor/caller clipping, plus the existing text/image/framebuffer tests. Native
macOS probes verify the layout gallery, z changes, a blocking overlay, inert
background removal from the accessibility tree, accessible close and restoration,
and the corrected centered label. Captures: [corrected layout](../images/layout-validation.png)
and [overlay](../images/overlay-validation.png). One-off probe copies live outside
the repository; the example has no test-only mode. This does not establish native
latency or Windows/Linux runtime coverage.
