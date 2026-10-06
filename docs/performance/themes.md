# Retained theme resolution CPU measurements

Recorded on 2026-10-06 on Apple M3 Pro/macOS, Rust 1.98.1 and Cargo's optimized
bench profile. Three runs enable default layout plus accessibility, without native
or rendering. The sources are uncommitted; exact SHA-256 fingerprints, platform
and reproduction details are in [themes/metadata.json](themes/metadata.json).
The development machine was interactive, without CPU affinity or isolated load.

```sh
cargo bench -p rxui --bench elements --features accessibility --locked
```

Cases warm for 20 operations and time 20 batches of 100. Values are microseconds
per operation averaged within each batch. Median ranges span the three runs;
the p95 column is the largest run's p95 batch average. These are not individual
event latencies. Nanosecond-scale idle/cache results are sensitive to compiler
optimization and timer amortization.

| Workload | Labels | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: | ---: |
| Idle prepare | 16 | 0.035–0.036 | 0.038 |
| Unchanged description rebuilt | 16 | 6.393–6.783 | 7.112 |
| Keyed reorder | 16 | 9.387–9.595 | 9.991 |
| Scroll geometry | 16 | 0.817–0.895 | 0.960 |
| One label changed | 16 | 9.283–9.728 | 10.296 |
| Dark/light palette switch | 16 | 0.833–0.917 | 0.961 |
| Default font size switch | 16 | 6.135–6.518 | 6.925 |
| Unchanged AccessKit cache update | 16 | 0.008–0.009 | 0.010 |
| Initial AccessKit tree | 16 | 6.664–7.034 | 7.495 |
| Idle prepare | 256 | 0.036–0.038 | 0.038 |
| Unchanged description rebuilt | 256 | 98.123–100.300 | 101.885 |
| Keyed reorder | 256 | 138.451–139.636 | 142.544 |
| Scroll geometry | 256 | 12.137–13.014 | 13.340 |
| One label changed | 256 | 133.579–136.987 | 139.115 |
| Dark/light palette switch | 256 | 10.382–11.268 | 11.393 |
| Default font size switch | 256 | 102.380–103.686 | 105.659 |
| Unchanged AccessKit cache update | 256 | 0.009–0.009 | 0.009 |
| Initial AccessKit tree | 256 | 95.307–96.132 | 99.248 |
| Idle prepare | 1,000 | 0.035–0.036 | 0.037 |
| Unchanged description rebuilt | 1,000 | 386.430–387.282 | 397.195 |
| Keyed reorder | 1,000 | 544.550–549.615 | 559.581 |
| Scroll geometry | 1,000 | 50.489–51.692 | 53.801 |
| One label changed | 1,000 | 534.567–541.050 | 549.542 |
| Dark/light palette switch | 1,000 | 41.901–45.113 | 45.653 |
| Default font size switch | 1,000 | 414.728–427.273 | 431.434 |
| Unchanged AccessKit cache update | 1,000 | 0.009–0.009 | 0.010 |
| Initial AccessKit tree | 1,000 | 383.709–389.834 | 399.212 |

## Theme workload boundary

The fixture contains keyed text labels in one retained root column. Palette switch
alternates preconstructed dark/light themes, installs the new root theme and
prepares the UI. Both themes use identical metrics. Counters assert no component
evaluation, text measurement or Taffy layout during the timed case. The entire
retained tree resolves paint bindings; no Taffy style replacement is needed.
Explicit scopes/overrides and control state data are covered by separate behavior
tests, not this label-only timing fixture.

Default font size switch alternates 16 and 18 logical units with the same palette.
It resolves inherited fonts, changes relevant text revisions and remeasures/layouts
the column, without view evaluation. Text measurement is deterministic UTF-8 byte
length times font size / 2, with height font size * 1.25. At the 16-unit default it
matches the previous baseline formula. This is real dirty-layout work with mock
text metrics, not actual shaping/rasterization or a font-loading benchmark.

Idle preparation still skips style traversal, description construction, text
measurement and layout. Unchanged descriptions compare styling along with the
existing keyed properties; the figures show the total reconciliation workload,
not isolated style comparisons. Single-label update still describes/reconciles
the entire root collection. Component boundaries and virtualization remain needed
for large UI models; styling does not make those collection operations constant-time.

Style state changes are not timed here. Behavior tests verify hover/pressed/focus
select cached paint values without new style resolution, measurement or layout.
A root palette switch resolves O(retained elements); local style changes resolve
the affected subtree. Retained nodes store base paint values, and controls also
store three cached pointer/disabled appearances. This trades some memory for cheap
state changes. Allocation counts and retained-memory totals are not measured here.

## Rendering and native checks

Independent GPU tests verify palette changes preserve the shaped TextLayout Arc,
interaction storage, glyph geometry, uploads and cache misses. A font-size change
rejects stale GPU preparation and creates the appropriate replacement layout.
Pixel readback validates rounded fills and uniform borders using both presets;
painting restores caller scissor state. Selection foreground reuses prepared glyphs
with an additional clipped draw per visual selection rectangle when colors differ.
That draw/vertex-processing cost is outside this CPU benchmark and can grow with
text geometry and selection fragments.

A separate macOS gallery probe was operated through AXPress/AXValue without
activating its windows. Application theme changes updated the inheriting window;
an explicitly themed light window stayed light. Its own theme toggle changed its
appearance, inheritance could be restored, shared text survived switching and its
button updated the shared counter. Window captures were visually inspected and
the probe exited through AXPress. Native screenshots are linked from
[the styling contract](../styling.md). This is behavioral verification, not native
latency, end-to-end frame timing or Windows/Linux runtime coverage.

Earlier [accessibility measurements](accessibility.md), [host/scroll checks](host-scroll.md)
and [elements baseline](elements.md) retain their historical source fingerprints.
Raw current results: [run 1](themes/elements-run-1.csv),
[run 2](themes/elements-run-2.csv), [run 3](themes/elements-run-3.csv).
