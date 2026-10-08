# Per-frame CPU measurements

Recorded 2026-10-08 on Apple M3 Pro/macOS 27.0.1, Metal, Rust 1.99.0, optimized
Cargo bench profile. The baseline is `9e1492d` with the frame benchmark added
(`8e71d67`); the current state is `7ee8047`, after the optimizations listed below.
[Metadata and source fingerprints](frames/metadata.json) record the inputs, and
raw CSVs accompany the report. This is an interactive machine without isolated
load or CPU affinity. Ranges span the medians of three runs.

```sh
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench frame --features rendering,accessibility --locked
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench frame --features rendering,accessibility --locked -- --accessibility
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench state --features tasks --locked
```

## What is timed

`benches/frame.rs` runs what the native host does for one redraw, with a loaded
font and a headless device: the input or state change, `Ui::prepare` and
`UiPainter::prepare`. The fixture is an 800×600 column holding a text input, a
status label and a scroll area over a separate component of 100 or 1,000 button
rows (24 units each, about 23 on screen). `idle_encode` adds CPU encoding of the
composed frame; nothing else records, submits or presents. Each case warms 20
operations, then times 20 batches of 20.

| Case | What changes per operation |
| --- | --- |
| idle | Nothing; a caret-blink or expose redraw. |
| idle_encode | idle, then `compose` and `paint` into a dropped frame. |
| hover_move | Pointer alternates between two rows; hover state changes. |
| pointer_listener_move | Pointer moves within a row whose view has a pointer-move listener that calls `cx.unchanged()`. |
| wheel_scroll | One 24-unit wheel step, alternating direction. |
| type_character | Insert, then backspace, in the focused controlled input. |
| theme_switch | Dark and light themes alternate. |
| opacity_animation | The status label's opacity follows the frame time. |

`--accessibility` adds an `AccessKitTree::update` after each frame, as the host
does while an assistive technology is active.

## Results

Microseconds per operation, median ranges of three runs.

| Case | Rows | Baseline | Current | Current with accessibility (baseline) |
| --- | ---: | ---: | ---: | ---: |
| idle | 100 | 82.1–88.5 | 0.37–0.38 | 0.37–0.38 (82.5–83.3) |
| idle_encode | 100 | 152.6–154.9 | 17.6–17.8 | 17.7–17.7 (152.1–157.1) |
| hover_move | 100 | 86.5–87.2 | 1.19–1.20 | 1.19–1.21 (87.0–88.5) |
| wheel_scroll | 100 | 99.1–100.6 | 5.3–5.3 | 42.0–42.0 (195.5–215.7) |
| type_character | 100 | 113.1–114.1 | 18.3–18.5 | 55.2–55.5 (210.1–256.7) |
| theme_switch | 100 | 109.1–110.4 | 12.3–12.5 | 12.3–12.5 (109.3–113.7) |
| opacity_animation | 100 | 148.4–150.4 | 9.5–9.7 | 45.5–45.9 (247.5–265.7) |
| idle | 1,000 | 789.3–793.9 | 0.37–0.37 | 0.37–0.37 (790.1–801.7) |
| idle_encode | 1,000 | 1347.5–1388.1 | 37.6–37.8 | 37.5–39.8 (1343.2–1383.6) |
| hover_move | 1,000 | 802.9–853.9 | 3.2–3.3 | 3.2–3.2 (804.5–809.5) |
| wheel_scroll | 1,000 | 940.2–961.5 | 44.9–54.0 | 378.6–381.3 (1851.9–1957.1) |
| type_character | 1,000 | 935.3–943.6 | 51.1–51.6 | 383.1–385.8 (1854.7–1872.1) |
| theme_switch | 1,000 | 1034.6–1037.4 | 120.3–135.0 | 120.6–121.8 (1036.7–1095.8) |
| opacity_animation | 1,000 | 1363.6–1367.7 | 58.4–59.9 | 379.2–381.8 (2280.2–2296.8) |

`pointer_listener_move` has no baseline row because `cx.unchanged()` is new. The
same listener without it re-evaluates the 1,000-row view on every move: 545–546 µs
per move, against 3.1–3.5 µs now (55 µs and 1.2 µs at 100 rows).

The current wheel and theme rows vary between runs (44.9–54.0 and 120–135 µs);
three runs while developing measured 44.2–46.0 and 119–122 µs for the same paths. The opacity case measured 46.8–47.3 µs with that revision; the
added listener case runs before it and leaves differently reconciled rows, so
compare it only within this report.

### Existing benchmarks

Single runs of the unchanged benchmarks at the baseline and now, in µs. These
use mock text measurement (elements, interaction) or rectangles (compositing).

| Benchmark | Operation | Count | Baseline | Current |
| --- | --- | ---: | ---: | ---: |
| elements | unchanged_description | 1,000 | 561.5 | 410.3 |
| elements | scroll_geometry | 1,000 | 120.3 | 23.0 |
| elements | theme_palette_switch | 1,000 | 193.0 | 44.7 |
| elements | semantic_initial_tree | 1,000 | 649.4 | 313.3 |
| interaction | wheel_geometry_with_handle | 1,000 | 125.5 | 23.8 |
| interaction | controlled_split_drag_and_layout | 1,000 | 457.6 | 227.2 |
| compositing | direct_prepare_cached | 1,000 | 391.2 | 0.20 |
| compositing | one_group_update_prepare | 1,000 | 1199.9 | 462.8 |
| compositing | direct_encode_discard | 1,000 | 379.1 | 181.9 |

Submit-and-wait times are within run-to-run variation (many groups at 1,000:
41.7 ms before, 41.4 ms now); they are dominated by driver and GPU work.

State `dispose_task_owners` (create N owners with one pending task each, drop and
synchronize; three runs) went from 145–146 to 68–70 µs at 256 owners and from
1161–1174 to 275–281 µs at 1,024.

## Where the time went

Sampling the baseline showed over 80% of every frame in `UiPainter::prepare`
building an `ElementInfo` (with an ancestor walk for focusability) for every node,
three times. With that gone, SipHash on `ElementId` lookups was about 40%. The
commits, in order:

| Commit | Change | Main effect at 1,000 rows |
| --- | --- | --- |
| `ddc3c45` | Prepare text from node data in one pass | idle 802–836 → 48–52 µs |
| `876f5b5` | Fx hasher for element-keyed maps | idle 48–52 → 16.5–16.8 µs; wheel 196–205 → 44 µs |
| `2f25750` | Cull off-screen elements before building `ElementInfo` in paint | idle_encode 227 → 53 µs |
| `5906c50` | Plan layers from node data; bound only groups | opacity_animation 229–234 → 47 µs |
| `113f766` | Clean partial trees only after interrupted reconciliation | type_character 78–80 → 51 µs |
| `88a60c6` | Skip the text pass while a placement is unchanged | idle 16.3–16.8 → 0.37 µs; hover 19 → 3.2 µs |
| `8651582` | `Context::unchanged` and `Dispatch::Unchanged` | no-op pointer listener 545 → 3.2 µs |
| `14066cb` | Cancel disposed owners' tasks in one pass | 1,024 owners 1.16 → 0.28 ms |
| `7ee8047` | AccessKit diff compares once; Fx-hashed node ids | accessibility wheel 540–565 → 379–382 µs |

## Remaining costs and limits

- With accessibility active, any evaluation, layout or geometry change still
  rebuilds and diffs the whole AccessKit snapshot: about 330 µs of a 1,000-row
  wheel or typing frame. Limiting it to changed subtrees needs per-node semantic
  revisions.
- Wheel scrolling and typing walk every node in `update_bounds` after layout.
  Taking child lists instead of cloning them in that walk measured no gain here.
- A theme switch resolves styles for every node (about 100 µs at 1,000 rows); it
  is a rare event and was left alone.
- Text for off-screen rows is still shaped and prepared when it changes. Painting
  skips it; preparing lazily would move that work into scroll frames.
- These are CPU timings. They do not include surface acquisition, submission,
  presentation, the native event loop or per-window bookkeeping (`record_mounts`
  still collects mount identities each frame, linear in mounted components).
