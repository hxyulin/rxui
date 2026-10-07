# Input and retained control CPU measurements

Recorded 2026-10-07 on Apple M3 Pro/macOS, Rust 1.98.1, optimized Cargo bench
profile, three runs per workload. [Metadata and source fingerprints](interaction/metadata.json)
record the inputs. Raw CSVs accompany the report. This is an interactive machine,
without isolated load or CPU affinity. Median ranges span three runs; largest p95
is the largest 95th-percentile batch average across runs, not individual-event latency.

```sh
CARGO_INCREMENTAL=0 cargo bench --bench interaction --locked
CARGO_INCREMENTAL=0 cargo bench --bench elements --locked
```

## Retained controls

The fixture is an 800×300 controlled split containing a scrollbar/viewport and a
separately mounted rows component. Counts denote 24-unit labels, with deterministic
mock sizing. The other pane is empty. Each case warms 20 operations, then times
20 batches of 50. It excludes real shaping, glyph resources, GPU work and presentation.

| Operation | Rows | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: | ---: |
| Idle prepare with handle | 16 | 0.141–0.150 | 0.184 |
| Wheel + geometry + metrics | 16 | 1.640–1.726 | 1.929 |
| Captured scrollbar drag + prepare | 16 | 3.243–3.258 | 3.471 |
| Controlled split drag + layout | 16 | 17.880–18.039 | 18.632 |
| Idle prepare with handle | 256 | 0.143–0.161 | 0.206 |
| Wheel + geometry + metrics | 256 | 13.913–14.062 | 14.540 |
| Captured scrollbar drag + prepare | 256 | 10.081–10.317 | 10.830 |
| Controlled split drag + layout | 256 | 106.458–107.222 | 110.405 |
| Idle prepare with handle | 1000 | 0.142–0.157 | 0.159 |
| Wheel + geometry + metrics | 1000 | 56.301–56.999 | 58.160 |
| Captured scrollbar drag + prepare | 1000 | 34.439–34.657 | 35.967 |
| Controlled split drag + layout | 1000 | 382.694–386.410 | 390.583 |

Idle preparation includes scroll-handle checks and asserts no counter changes.
Wheel motion alternates one unit; captured scrollbar motion alternates two nearby
positions and includes pointer routing, hit testing, thumb/range calculations,
geometry/metric publication and preparation. Both scroll cases assert **no view
evaluation, text measurement, node allocation or Taffy layout**. Scrollbars change
retained geometry directly rather than requiring an application listener.

Split motion alternates one unit and includes its listener, controlled model update,
root description reconciliation, dirty Taffy layout and geometry/metric publication.
Counters assert one root evaluation and layout pass per operation, stable nodes,
and no reevaluation of the separately mounted rows description. Text can still be
measured when width changes; retaining descriptions does not eliminate layout/reflow.

At 1,000 rows these CPU paths are roughly 0.057 ms for wheel geometry, 0.035 ms
for captured scrollbar motion with preparation, and 0.39 ms for controlled split
resizing. Their different targeting paths make the wheel/drag figures unsuitable
for a claim that dragging is inherently faster. These are not GPU timings or proof
of a complete frame/input-to-presentation budget. All rows are retained; geometry
refresh still scales with the tree, and virtualization remains future work.

## Existing elements regression check

The unchanged elements benchmark ran three times against the committed opacity
baseline `6a06f05` and three times against this work, on the same machine and with
the same feature/profile settings. Baseline ran first, so order/load variation
remains a confounder. Representative 1,000-row median ranges:

| Operation | Committed baseline (µs) | Current (µs) |
| --- | ---: | ---: |
| idle_prepare | 0.039–0.039 | 0.070–0.071 |
| unchanged_description | 499.515–507.168 | 496.151–498.333 |
| keyed_reorder | 665.815–673.461 | 659.366–661.261 |
| scroll_geometry | 51.916–52.743 | 53.275–55.796 |
| single_label_update | 649.592–656.881 | 644.305–645.978 |

Unchanged descriptions, reorder and text updates remain comparable in these runs;
the differences do not establish a speedup. Idle preparation adds around 30 ns in
this fixture for new control/queued-work checks, remaining below 0.1 µs. Scroll
geometry stays close to the baseline. This does not measure real font reflow,
allocation counts, custom-listener-heavy trees or resize storms across many windows.

`Element` is 912 bytes on this target, up from 896: optional input/scroll-reference
handles add 16 inline bytes. Pointer/key listener properties are in an optional
box; ordinary elements allocate none. Built-in range settings are boxed only on
scrollbar/divider nodes. Scroll references/metric subscriptions allocate only when
used. Warm routing reuses its ancestor vector; scalar pointer payloads need no
heap allocation. These observations are storage/code-path facts, not an allocator
instrumentation result.

## Behavioral verification

CPU tests cover route order, propagation versus defaults, custom focus, three
mouse buttons, capture outside bounds, cancellation on Escape/removal/inertness/
deactivation, split constraints and desired-size preservation, vertical splits,
axis changes, range keys, duplicate handles, weak/stale commands, placement isolation,
metric subscriptions/unsubscription, viewport/content changes and feedback-loop
retry. AccessKit tests verify numeric actions and physical orientation at DPR 2.

An opt-in Metal readback checks thumb/line pixels, scrolling through a group-opacity
layer, clipping, caller scissor restoration and a resized split submission. The
complete existing GPU suite also passes.

Native checks use a one-off copied example, with no smoke mode in shipped examples.
macOS AX value changes resized the sidebar from 240 to 320 and scrolled files to
400; pointer dragging resized it to 384, and Escape restored 384 after a second
drag. Thumb capture continued after leaving its track (offset about 1,929), while
the output viewport remained at zero. A second window showed the shared sidebar
size 384 and independent zero file offset. A card dragged beyond its pane returned
to its original native bounds on Escape. This checks native action/input integration,
not full VoiceOver announcement/navigation usability.

- [Initial workspace capture](../images/workspace-controls-initial.png)
- [After resizing/scrolling and card Escape](../images/workspace-controls-dragged.png)

The portable feature check also exposed a pre-existing Cargo boundary: native
async-io timers were included on wasm despite the code being native-only. The timer
dependency/import now use the same non-wasm condition. Desktop feature behavior is
unchanged; portable layout/rendering/accessibility/tasks compile on wasm, without
promising a browser host or built-in wasm task executor.
