# Accessibility cache and idle preparation measurements

Recorded on 2026-10-06 on Apple M3 Pro/macOS with Rust 1.98.1 and Cargo's optimized
bench profile. Three runs enable default layout plus accessibility; native and
rendering features are disabled. The sources were uncommitted, identified by the
SHA-256 fingerprints in [accessibility/metadata.json](accessibility/metadata.json).
The machine was interactive, without CPU affinity or isolated load.

```sh
cargo bench -p rxui --bench elements --features accessibility --locked
```

Each case warms for 20 operations and records 20 batches of 100 operations.
Values are microseconds per operation averaged inside each timed batch. Median
ranges span three runs; the p95 column is the largest run's p95 batch average.
These are not individual-event latency percentiles. Very small idle/cache timings
are sensitive to optimization and timer amortization and do not predict OS event
latency.

| Workload | Labels | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: | ---: |
| Idle prepare | 16 | 0.030–0.034 | 0.035 |
| Unchanged description rebuilt | 16 | 6.342–6.512 | 6.873 |
| Keyed reorder | 16 | 9.154–9.170 | 9.517 |
| Scroll geometry | 16 | 0.828–0.891 | 0.973 |
| One label changed | 16 | 9.037–9.189 | 9.786 |
| Unchanged AccessKit cache update | 16 | 0.009–0.009 | 0.010 |
| Initial AccessKit tree | 16 | 7.093–7.183 | 7.930 |
| Idle prepare | 256 | 0.030–0.032 | 0.033 |
| Unchanged description rebuilt | 256 | 94.004–95.059 | 96.151 |
| Keyed reorder | 256 | 133.472–135.049 | 136.462 |
| Scroll geometry | 256 | 11.888–12.198 | 12.638 |
| One label changed | 256 | 130.327–131.381 | 134.033 |
| Unchanged AccessKit cache update | 256 | 0.009–0.009 | 0.010 |
| Initial AccessKit tree | 256 | 98.403–99.824 | 100.805 |
| Idle prepare | 1,000 | 0.030–0.033 | 0.033 |
| Unchanged description rebuilt | 1,000 | 371.672–376.682 | 379.924 |
| Keyed reorder | 1,000 | 526.543–530.415 | 533.623 |
| Scroll geometry | 1,000 | 48.003–48.838 | 49.717 |
| One label changed | 1,000 | 513.440–515.805 | 518.467 |
| Unchanged AccessKit cache update | 1,000 | 0.009–0.009 | 0.009 |
| Initial AccessKit tree | 1,000 | 394.084–397.242 | 403.162 |

## What the cases establish

The fixture is a root view containing keyed labels in a column. Text measurement
is a deterministic byte-length formula. Initial AccessKit tree timing constructs,
translates and drops a fresh cache/tree on each iteration, including allocation
and value copying. It does not include Ui preparation, platform tree consumption,
assistive-technology queries or text-input grapheme encoding. At 1,000 labels this
cost is around 0.4 ms. It scales with retained content and is not a virtualized-list
measurement.

The unchanged AccessKit case begins with a prepared/published tree, then calls
update repeatedly with identical placement state, title and scale. It returns None;
counters assert zero additional node or text work. The retained revision check is
independent of label count in this fixture. Publication is disabled in Application
until an adapter requests activation, so inactive native windows do not build
these trees.

Idle Ui preparation now uses a retained input index instead of cloning/traversing
all element IDs to discover text inputs. With no inputs, child components, dirty
descriptions or constraint/font changes, its cost is constant across label counts
in this fixture. Idle preparation with inputs still refreshes input scrolling;
mounted child components still require dependency checks. These measurements do
not imply every interface has a constant-time idle preparation path.

The other cases preserve the existing workload boundaries: root description
construction/reconciliation scales with its collection; reorder retains IDs;
a single-label update still rebuilds the root description. Scroll refreshes
retained geometry without rebuilding descriptions, running Taffy or remeasuring
text, but remains proportional to retained elements. Assertions check those cache
and identity properties. Earlier [elements](elements.md) and
[host/scroll](host-scroll.md) reports retain their historical source fingerprints.

Changed semantic publication currently scans/translates the retained semantic
tree before diffing and sends only changed nodes. An O(1) unchanged check is not
an O(1) changed-tree guarantee. Text-input consumer tests, separately from this
label benchmark, verify that selection changes publish the field parent while
reusing the TextRun and its cached grapheme boundaries; hover/caret blink publishes
nothing. Oversized grapheme fallback and stale text-run rejection are also tested.

## Native verification and remaining boundary

A separate one-off native probe on macOS exposed named AXTextField and AXButton
nodes through System Events. AXPress updated a counter to 1; AXValue submitted
native edit through the application's uppercase controlled handler and returned
NATIVE EDIT. Its close button was invoked through AXPress and the process exited
successfully. This was a behavioral check, not a latency benchmark or a full
VoiceOver pass. It did not interact with the user's existing windows.

This report excludes real shaping/rasterization, text-input editing throughput,
GPU preparation/recording/presentation, native adapter processing, screen-reader
latency, allocation counts and Windows/Linux runtime verification. The existing
GPU tests separately validate cached text reuse, input selection/IME draws and
clip/scissor pixels. Per-character accessibility geometry remains future work.

Raw results: [run 1](accessibility/elements-run-1.csv),
[run 2](accessibility/elements-run-2.csv), [run 3](accessibility/elements-run-3.csv).
The [semantics contract](../semantics.md) documents the API and host lifecycle.
