# Initial headless state baseline

This historical baseline corresponds to the state-only foundation committed in
`108de04`. New declarative-core sources have separate measurements; its source
fingerprints below identify the original run rather than the current tree.

Recorded on 2026-10-06 on Apple M3 Pro/macOS with Rust 1.98.1, the default
headless feature set, and the optimized Cargo bench profile. Three consecutive
runs were made after compilation completed. Raw CSV and source/environment
fingerprints are in [state/](state/metadata.json).

Reproduce with:

```sh
cargo bench -p rxui --bench state --locked
```

Each workload warms up for 1,000 operations, then measures 40 batches of 10,000
operations. The table gives the range of the three runs' median batch averages
and the largest p95 batch average. These are nanoseconds per operation averaged
within batches, not individual-operation latency percentiles. They include the
application update boundary where applicable.

| Operation | Dependent mounts | Median range (ns) | Largest p95 (ns) |
| --- | ---: | ---: | ---: |
| Scalar entity update | 0 | 7.1–7.2 | 8.2 |
| Update and invalidate | 1 | 32.4–35.6 | 36.7 |
| Update and invalidate | 16 | 203.6–205.7 | 211.8 |
| Update and invalidate | 256 | 2,844.7–2,877.9 | 2,963.4 |
| Bound listener dispatch/update | 1 | 62.8–64.9 | 70.2 |
| Update and evaluate tracked model | 1 | 232.5–243.5 | 267.0 |
| Update, notify one observer and flush | 0 | 90.1–94.2 | 108.7 |

The scalar cases use a `u32` and a small callback. The invalidation-only cases
establish dependencies once, then repeatedly update while mounts remain dirty.
They exercise repeated source invalidation, without description reconstruction.
The update/evaluate case tracks one model from a separate unit-state mount and
replaces that dependency set each iteration. The listener is created once and
dispatched repeatedly; listener construction is not timed. The observer case
holds one subscription and flushes after every update.

Steady-state dependency buffers are reused, and pending source notification
dispatch does not allocate a separate boxed callback for each notification.
Invalidation visits the source's dependent mounts on every update, even if they
are already dirty. Its cost therefore grows with the number of dependents.
Dirty flags and pending source notifications coalesce work, but do not imply
constant-time invalidation of a large dependency set. No allocator instrumentation
was used, so this report does not establish an allocation count.

This gives a useful initial regression baseline for the implemented state layer.
It does not measure keyed reconciliation, Taffy layout, large domain models,
description/listener construction, text shaping, rasterization, GPU preparation,
painting, native event processing, presentation or end-to-end input latency.
There is no measured comparison with the previous RXUI implementation. The next
milestone should add consumer workloads that measure those costs separately,
particularly shared models, unchanged descriptions and keyed list updates.

Behavior is checked independently through unit and compile-fail tests for live
listeners, ownership, stale handles, dependency replacement, unwind safety,
deferred observers, bounded cycles and access capabilities. A low timing value
does not replace those guarantees.
