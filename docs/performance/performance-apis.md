# Measurement equality and view deadlines

The unchanged architecture probe from `ae08bf2` was rerun after adding
`CustomElement::measure_eq` and view deadline wakeups on the same heavily loaded
Apple M5 Max host. The imported probe is kept in its own commit. Reproduce with:

```sh
cargo bench -p rxui --bench architecture_probe --features accessibility --locked
```

The measurement boundaries match the [original probe](architecture-probe/metadata.json):
mock text measurement, headless layout, 20 warmup operations and 400 sampled
operations for spinner frames, and 30 frames at 16 ms intervals for tooltip waiting.
The [raw CSV](performance-apis/probe-run-1.csv) and
[work counters](performance-apis/probe-counters.log) retain the complete rerun.
Compare with the [original counters](architecture-probe/probe-counters.log).
Timings are contended and do not establish a speedup; the conclusions below use
operation counts only.

With 1,000 labels, both the owner spinner and isolated child spinner add **zero
layout passes**, compared with 80 previously. The built-in glyph's intrinsic
measurement is independent of spinner phase. The owner still reconciles 420,840
nodes over 420 evaluations; the isolated child reconciles 420. This change removes
layout invalidation, not whole-view evaluation or description construction.

During the 480 ms tooltip wait, owner evaluations fall from 30 to **one**, and
reused nodes from 30,090 to 1,003. That one evaluation establishes fresh entry
time at 16 ms and requests the 616 ms deadline. No animation frame remains
requested after waiting. The deadline's actual firing and cancellation are covered
by deterministic tests; this probe ends before it fires.

The custom-element regression test separately counts measurement calls across
80 description phase/color updates: **zero additional measurements and zero
layout passes**. Hit testing, resolved color and semantic labels follow the new
description. Size, font, constraint and custom-type changes retain normal layout
invalidation. An opt-in GPU regression checks changed custom paint output while
measurement is reused.

Deadline tests cover successful replacement and omission, early unrelated frames,
failed evaluations (returned failure and unwind), one-shot firing, dirty work
retained without preparation, independent placements of a shared owner, nested
component placement queries, unmount and cascading local-state cleanup. Tooltip
tests cover idle waiting, firing, leave, suppression and omission. Native timer
aggregation has a deterministic caret/view minimum test. Surface availability
relies on Astrelis's retained dirty scheduling; native integration queues expired
work without repeatedly arming an expired timer. Interactive surface-loss behavior
is not benchmarked here.

Accessibility rebuilding, layer recording, text copying/shaping and virtualization
subscriptions retain their existing costs. No improvements to those paths are
claimed. Deadline advancement scans requesting mounts only; placement queries
scan their mounts and global deadline queries scan active requests.
