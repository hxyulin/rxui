# Fixed-height list scaling

Measured on 2026-10-07 on Apple M3 Pro with Rust 1.99.0, release profile:

```sh
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench virtual_list --locked
```

The committed benchmark uses an 800×600 logical viewport, 28-pixel rows and two
overscan rows on either side. It measures CPU description conversion,
reconciliation, retained geometry and Taffy layout with deterministic mock text
sizing. It excludes actual shaping/rasterization, GPU preparation/submission,
native accessibility publication, event dispatch and display frame latency.
Each operation has 20 warmups and 50 samples of 20 iterations; p95 selects the
48th sorted sample. Initial_prepare creates a fresh Runtime/Ui each iteration.

| Total rows | Live elements | Initial prepare median/p95 µs | Row crossing median/p95 µs | Within row median/p95 µs | Idle median/p95 µs |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 52 | 84.640 / 88.125 | 44.452 / 49.981 | 34.590 / 36.375 | 0.117 / 0.140 |
| 10,000 | 52 | 81.571 / 85.325 | 43.592 / 44.844 | 34.925 / 36.983 | 0.123 / 0.146 |
| 100,000 | 52 | 81.423 / 85.031 | 43.487 / 45.298 | 34.419 / 37.496 | 0.108 / 0.142 |

These results support bounded retained node count and CPU layout work at a fixed
viewport. The benchmark asserts that within-row scrolling adds no layout passes
or text measurements. It still reevaluates the subscribing view on scroll metric
changes; a native frame includes work outside this benchmark. This is not a claim
about native FPS or GPU execution time, and it is not a comparison to an eager
100,000-row render with real fonts.

Tests also cover distant jumps, keyed overlap/data insertion, fractional offsets,
count shrink/empty lists, zero-height viewport/resize, independent shared windows,
reveal_row, focus removal and invalid/extreme extents. AccessKit consumer validation
checks bounded mounted trees with total/index metadata after a distant jump.
A GPU readback regression checks slot colors after a large fractional jump with
4× MSAA and isolated group opacity. Native interaction remains platform-specific.
