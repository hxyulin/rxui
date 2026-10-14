# Virtual list scaling

Measured on 2026-10-15 on an Apple M5 Max with macOS 27.0.1, Rust 1.99.0, release
profile:

```sh
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench virtual_list --locked
```

The committed benchmark uses an 800×600 logical viewport and two overscan rows on
either side. Fixed rows are 28 pixels; variable rows cycle through 20, 28 and 36
pixels, also averaging 28. It measures CPU description conversion, reconciliation,
retained geometry and Taffy layout with deterministic mock text sizing. It excludes
actual shaping/rasterization, GPU preparation/submission, native accessibility
publication, event dispatch and display frame latency. Each operation has 20
warmups and 50 samples of 20 iterations; p95 selects the 48th sorted sample.
Initial_prepare creates a fresh Runtime/Ui each iteration, including, for variable
rows, collecting the `VirtualRows`. The benchmark never starts the animation clock,
so scrolling is immediate.

The baseline is `1d5e6a2` with its own benchmark source. The candidate adds
variable-height lists; its benchmark keeps the fixed-height operations unchanged and
adds `variable_` operations after them. The machine was interactive with other
builds running, so the two binaries ran alternately, three runs each. Ranges span
the medians of those runs, in microseconds. Raw CSVs are in
[virtual-lists/](virtual-lists): `baseline-run-N.csv` and `variable-run-N.csv`.

## Fixed heights

| Total rows | Live elements | Initial prepare base / current | Row crossing base / current | Within row base / current | Idle base / current |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 52 | 46.65–57.85 / 47.14–63.64 | 25.56–26.32 / 26.18–27.38 | 19.74–20.13 / 20.23–20.62 | 0.077–0.083 / 0.071–0.077 |
| 10,000 | 52 | 45.52–46.57 / 46.49–47.21 | 25.39–25.68 / 25.99–26.53 | 19.74–20.52 / 20.24–20.68 | 0.077 / 0.071 |
| 100,000 | 52 | 45.58–46.37 / 46.50–47.27 | 25.45–26.06 / 26.06–26.58 | 19.80–20.28 / 20.34–20.63 | 0.077 / 0.071 |

The fixed-height list's code path is unchanged apart from sharing its slot
construction with the variable list. The current medians are 0–4% above the
baseline, within the spread between runs on this machine; a rerun of the
baseline benchmark source against the candidate library showed the same overlap.
The first 100-row initial_prepare run of each binary is an outlier.

## Variable heights

| Total rows | Initial prepare | Row crossing | Within row | Idle | Resize above | Insert above |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 46.72–47.58 | 25.92–26.41 | 20.84–21.21 | 0.071 | 52.32–53.23 | 53.92–55.03 |
| 10,000 | 83.72–85.38 | 25.87–26.57 | 20.83–21.16 | 0.071–0.073 | 52.15–53.47 | 86.07–87.68 |
| 100,000 | 404.08–410.03 | 25.98–26.42 | 20.90–21.13 | 0.067–0.071 | 53.34–54.40 | 381.53–386.25 |

Scrolling a variable-height list costs the same as a fixed one at every size: the
O(log n) row lookups do not show against reconciliation and layout. Initial
preparation grows with the row count only because the benchmark builds the
`VirtualRows` (keys, heights and the Fenwick tree) on every iteration; that is
model construction, which an application does once.

Resize_above changes the height of row 0 while the viewport is in the middle of the
list and prepares; anchoring moves the offset by the same amount, so the mounted
rows keep their identities and positions on screen. It stays constant with size
because `set_height` is O(log n). Insert_above alternately inserts and removes a
row at index 0 at the same position. Each change rebuilds the Fenwick tree and
shifts the key and height vectors, O(n); the anchor finds its row in O(1) because
only the length changed. The asserted offset after an even number of operations
equals the starting one.

These results support bounded retained node count and CPU layout work at a fixed
viewport. The benchmark asserts that within-row scrolling adds no layout passes
or text measurements. It still reevaluates the subscribing view on scroll metric
changes; a native frame includes work outside this benchmark. This is not a claim
about native FPS or GPU execution time, and it is not a comparison to an eager
100,000-row render with real fonts.

Tests also cover distant jumps, keyed overlap/data insertion, fractional offsets,
count shrink/empty lists, zero-height viewport/resize, independent shared windows,
reveal_row, focus removal and invalid/extreme extents. Variable-height tests cover
Fenwick prefix sums and lookups against a linear scan, edits, anchoring on
insertion, removal and resizing above the viewport, removal of the anchor row, the
cases that skip anchoring, reveal_list_row and invalid heights. AccessKit consumer
validation checks bounded mounted trees with total/index metadata after a distant
jump. A GPU readback regression checks slot colors after a large fractional jump
with 4× MSAA and isolated group opacity. Native interaction remains
platform-specific.
