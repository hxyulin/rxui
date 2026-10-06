# Declarative core CPU baseline

Recorded on 2026-10-06 on Apple M3 Pro/macOS, Rust 1.98.1, optimized Cargo bench
profile, default layout feature. This is the historical baseline before native hosting/tasks/scrolling.
The later [host and scrolling check](host-scroll.md) records the native-host slice;
[accessibility measurements](accessibility.md) record the editing/semantics slice. Three runs use the measured source fingerprints in [elements/metadata.json](elements/metadata.json). The machine was
an interactive development machine, without CPU affinity or an isolated load;
the ranges include scheduling variation. GPU/native features were disabled.

Reproduce with:

```sh
cargo bench -p rxui --bench elements --locked
cargo bench -p rxui --bench state --locked
```

The elements benchmark warms up for 20 operations, then times 20 batches of 100
operations per case. Values below are microseconds per operation averaged inside
each batch. Median ranges span the three runs; p95 is the largest run's p95 batch
average. These are not individual-event latency percentiles.

| Workload | Labels | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: | ---: |
| Idle prepare | 16 | 0.033–0.035 | 0.035 |
| Unchanged description rebuilt | 16 | 6.497–6.707 | 7.117 |
| Keyed reorder | 16 | 9.188–9.284 | 9.711 |
| One label changed | 16 | 9.009–9.352 | 9.621 |
| Idle prepare | 256 | 0.031–0.036 | 0.037 |
| Unchanged description rebuilt | 256 | 94.731–95.909 | 98.948 |
| Keyed reorder | 256 | 135.445–154.501 | 177.256 |
| One label changed | 256 | 131.044–141.174 | 177.665 |
| Idle prepare | 1,000 | 0.031–0.040 | 0.040 |
| Unchanged description rebuilt | 1,000 | 381.169–412.666 | 483.396 |
| Keyed reorder | 1,000 | 533.594–565.567 | 641.705 |
| One label changed | 1,000 | 518.000–633.378 | 795.347 |

The root View describes a flex column containing all labels with integer keys.
Descriptions allocate owned strings and a child collection. Unchanged-description
work conservatively dirties the root with a no-op update, then constructs,
validates and reconciles the same description. Counters verify no extra text
measurement or Taffy layout computation. Keyed reorder rotates the collection;
counter assertions verify no new or removed node identities. One-label update
changes the text for key zero, while still rebuilding the root's entire description.

Idle preparation has no child components, dirty description, changed constraints
or external measurement generation. It skips reconstruction, paint-order traversal
and layout, explaining its tiny cost. With mounted child components, idle dirty
checking scans those components. None of these idle timings includes the work
required to paint a newly acquired surface.

Text measurement is a deterministic byte-length formula, not real shaping.
Changed text triggers relevant leaf measurement, but root description construction,
reconciliation and column layout remain proportional to collection size. This
baseline is not a virtualization or large-document scalability result. It supports
continuing with measured consumer workloads and component boundaries, rather than
assuming a one-field update makes a large root description constant-time.

The matching state-runtime runs place bound listener dispatch at 46.8–54.5 ns
and update plus tracked evaluation at 230.3–239.4 ns median batch averages.
Variation and optimizer effects in these small cases prevent claiming a speedup
over the [initial state baseline](state-runtime.md). The declarative integration
has not introduced a measured order-of-magnitude increase in those state cases.

Raw results are [elements run 1](elements/elements-run-1.csv),
[run 2](elements/elements-run-2.csv), [run 3](elements/elements-run-3.csv),
and matching [state run 1](elements/state-run-1.csv),
[run 2](elements/state-run-2.csv), [run 3](elements/state-run-3.csv).
No real font shaping, rasterization, atlas uploads, GPU preparation/recording,
native event processing, presentation or allocation counts are timed here.

Independent behavior checks cover unchanged measurement/layout, keyed identity,
child dependencies, removal/disposal, focus/capture, retry after errors/panic and
reflow. An opt-in native GPU test verifies color-only changes retain geometry and
glyph uploads, records actual UI draws, and rejects stale text resources before
recording. The window example was checked for keyboard activation, resize and
background completion. These checks establish the boundaries needed for later
end-to-end UI performance measurements.
