# Animation cost measurements

Recorded 2026-10-10 on an Apple M5 Max with macOS 27.0.1 and Metal, using Rust 1.99.0
and the optimized Cargo bench profile. The baseline is `db84b44`. The current state
is the `feat/animation` branch at `b20982e`. The benchmark sources are unchanged.
This is an interactive machine with other work running (load average about 6), so
the two builds ran alternately in ABBA order, four runs each. Ranges span the
medians of those runs. Raw CSVs are in [animation/](animation), named
`<bench>-<base|cur>-<run>.csv`; `frameax` is the frame bench with `--accessibility`.

```sh
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench frame --features rendering,accessibility --locked
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench frame --features rendering,accessibility --locked -- --accessibility
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench elements --locked
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench interaction --locked
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench compositing --features rendering --locked
```

The [frame benchmark](frames.md) calls `Runtime::begin_frame` with 16 ms steps, so
the animation clock runs. Its `hover_move` and `theme_switch` cases therefore start
real transitions on every operation, and those transitions are always in flight.
The other benchmarks never start the clock, so motion is immediate there.

## Results

Microseconds per operation.

| Bench | Case | Count | Baseline | Current |
| --- | --- | ---: | ---: | ---: |
| frame | idle | 1,000 | 0.265–0.269 | 0.279–0.310 |
| frame | idle_encode | 1,000 | 28.6–29.9 | 27.8–31.3 |
| frame | hover_move | 100 | 0.86–0.87 | 1.06–1.19 |
| frame | hover_move | 1,000 | 2.37–2.52 | 2.56–2.97 |
| frame | wheel_scroll | 1,000 | 32.5–34.2 | 32.2–38.0 |
| frame | type_character | 1,000 | 38.6–39.3 | 37.8–44.6 |
| frame | theme_switch | 100 | 9.4–12.0 | 13.1–15.6 |
| frame | theme_switch | 1,000 | 93.6–95.1 | 99.0–119.5 |
| frame | opacity_animation | 1,000 | 33.2–34.0 | 32.2–38.1 |
| frame + AccessKit | wheel_scroll | 1,000 | 292–308 | 296–360 |
| elements | idle_prepare | 1,000 | 0.063–0.063 | 0.071–0.077 |
| elements | unchanged_description | 1,000 | 326–334 | 326–365 |
| elements | scroll_geometry | 1,000 | 16.4–21.1 | 16.3–17.6 |
| elements | theme_palette_switch | 1,000 | 31.6–32.8 | 35.0–37.8 |
| interaction | idle_with_handle | 1,000 | 0.088–0.092 | 0.098–0.104 |
| interaction | wheel_geometry_with_handle | 1,000 | 16.7–17.0 | 16.8–17.7 |
| compositing | many_groups_update_prepare | 1,000 | 570–712 | 566–617 |

## What changed

- **Idle frames.** No frame is requested and no motion state is allocated. Each
  `Ui::prepare` still compares four interaction targets and reads the frame clock,
  which costs about 10 ns (0.27 to 0.28 µs per idle frame, 0.063 to 0.071 µs per idle
  preparation).
- **Hover.** About 0.2 µs per move for two paint transitions and advancing them.
  The painter blends paint only for the elements it draws.
- **Theme switch.** 5 to 20 µs at 1,000 rows. Each style resolution checks whether its
  theme changed, and every on-screen element starts a crossfade. Rows clipped out of
  view switch without one. `elements/theme_palette_switch` (no clock) pays only the
  per-node check, 3 to 5 µs.
- **Everything else.** Within run-to-run variation; the current build's upper ranges
  are wider on this loaded machine. An earlier build that stored the transition
  inline in `Element` grew every retained node by 8 bytes and measured +26% on
  `scroll_geometry` (17.0 to 21.3 µs). Keeping it in the boxed state styles removed
  that difference.

Overlay enter and exit motion is not part of these benchmarks. While an overlay
moves, each frame rebuilds the composition plan and records one layer pass for it,
as an animated group opacity does. That is the `one_group_update_prepare` path.
