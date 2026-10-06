# Native host and scrolling CPU check

Recorded on 2026-10-06 on Apple M3 Pro/macOS with Rust 1.98.1, optimized Cargo
bench profile, three runs per workload. [Metadata](host-scroll/metadata.json)
records source fingerprints, features and reproduction commands. The machine was
interactive, without CPU affinity or isolated load. Source fingerprints match the
measured working tree; changes were uncommitted when measured. This is the
historical host/scroll slice before editing/AccessKit; the later
[accessibility report](accessibility.md) records the updated elements workload.

This checks the retained geometry cost introduced by scrolling and compares the
previous declarative cases after the host/task additions. The elements benchmark
uses deterministic mock text sizes, not actual shaping. The state benchmark also
runs with all features enabled, but without configuring a host/executor or running
jobs. It measures state dispatch with those feature paths compiled in, not native
input, wakeup latency or background task throughput.

```sh
cargo bench -p rxui --bench elements --locked
cargo bench -p rxui --bench state --locked
cargo bench -p rxui --bench state --all-features --locked
```

Elements uses 20 batches of 100 operations after warmup. State uses 40 batches of
10,000 operations. Median ranges span the three runs; largest p95 is the maximum
p95 batch average across them. These are percentiles of per-operation batch
averages, not individual-event latency percentiles.

| Elements operation | Labels | Median range (µs) | Largest p95 (µs) |
| --- | ---: | ---: | ---: |
| Idle prepare | 16 | 0.033–0.037 | 0.037 |
| Unchanged description rebuilt | 16 | 6.303–6.601 | 7.030 |
| Keyed reorder | 16 | 9.113–9.357 | 9.832 |
| One label changed | 16 | 8.937–9.295 | 9.767 |
| Scroll geometry | 16 | 0.825–0.860 | 0.932 |
| Idle prepare | 256 | 0.035–0.040 | 0.040 |
| Unchanged description rebuilt | 256 | 93.843–95.618 | 97.635 |
| Keyed reorder | 256 | 134.785–137.373 | 138.851 |
| One label changed | 256 | 130.961–133.768 | 136.697 |
| Scroll geometry | 256 | 13.677–13.815 | 14.542 |
| Idle prepare | 1,000 | 0.039–0.041 | 0.050 |
| Unchanged description rebuilt | 1,000 | 375.060–382.460 | 386.191 |
| Keyed reorder | 1,000 | 535.527–540.048 | 545.442 |
| One label changed | 1,000 | 519.281–527.646 | 532.680 |
| Scroll geometry | 1,000 | 55.028–58.348 | 59.847 |

Scrolling alternates one logical pixel up/down within a 300×100 container, with
all rows retained. Assertions require actual offset changes and unchanged UI
counters: no view evaluation, measurement or Taffy layout computation. Hit routing,
ancestor motion consumption and geometry refresh are included. Cost grows with the
retained node count; 1,000 labels are around 0.055–0.058 ms for this CPU-only path.
This is not a virtualization result or a paint/presentation budget.

The unchanged/reorder/text cases remain in the range of the
[previous declarative baseline](elements.md), within the variation of these runs.
A changed root still reconstructs its entire description. There is no basis to
claim a speedup from scheduling variation; the observed changes do not show a
large CPU regression in these workloads.

| State operation | Feature set | Median range (ns) | Largest p95 (ns) |
| --- | --- | ---: | ---: |
| Bound listener | Default layout | 45.667–61.058 | 64.042 |
| Bound listener | All features, execution unconfigured | 48.650–65.925 | 68.171 |
| Update + tracked evaluation | Default layout | 222.383–230.846 | 238.617 |
| Update + tracked evaluation | All features, execution unconfigured | 233.200–242.517 | 249.946 |
| Update + observer flush | Default layout | 87.612–89.208 | 94.233 |
| Update + observer flush | All features, execution unconfigured | 88.017–94.392 | 98.971 |

Native hosting remains on demand. Unit tests verify coalesced completion wakeups,
bounded task draining, cancellation/disposal, and source-window routing for shared
models. A worker-pool test verifies timers progress while a blocking worker is
occupied. The interactive two-window example was confirmed to share model data
while retaining separate UI interaction state.

Two opt-in GPU tests separately verify retained text geometry/uploads, resources
for multiple Ui placements sharing one painter, stale preparation rejection,
caller scissor restoration, and actual pixel clipping before/after scrolling.
Those are behavior checks, not timed GPU/native performance measurements. No
allocation counts, real shaping, rasterization, upload latency, input-to-presentation
latency, or long-running task/large-list memory behavior is established here.

Raw data: [elements 1](host-scroll/elements-run-1.csv),
[2](host-scroll/elements-run-2.csv), [3](host-scroll/elements-run-3.csv);
[default state 1](host-scroll/state-run-1.csv),
[2](host-scroll/state-run-2.csv), [3](host-scroll/state-run-3.csv);
[all-features state 1](host-scroll/state-native-run-1.csv),
[2](host-scroll/state-native-run-2.csv), [3](host-scroll/state-native-run-3.csv).
