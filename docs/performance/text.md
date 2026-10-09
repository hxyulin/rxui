# Text area editing measurements

Recorded 2026-10-10 on Apple M3 Pro/macOS 27.0.1, Metal, Rust 1.99.0, optimized
Cargo bench profile, on the `feat/text` branch. [Metadata and source
fingerprints](text/metadata.json) record the inputs; raw CSVs accompany the report.
The machine was interactive and shared with other builds, without isolated load or
CPU affinity. Ranges span the medians of three runs.

```sh
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench text --features rendering,accessibility --locked
CARGO_INCREMENTAL=0 cargo bench -p rxui --bench text --features rendering,accessibility --locked -- --accessibility
```

## What is timed

`benches/text.rs` runs one redraw's CPU work with a loaded font and a headless
device: the edit or caret movement, `Ui::prepare` and `UiPainter::prepare`. The
fixture is an 800×600 scrolling column holding a controlled `text_area` of 100,
1,000 or 5,000 lines of 61 bytes ("Line 00042: the quick brown fox…"), about 6 KB,
61 KB and 305 KB. Each case warms 10 operations, then times 20 batches of 10.

| Case | What changes per operation |
| --- | --- |
| idle | Nothing; a caret-blink or expose redraw. |
| caret_vertical | Down, then Up, from the middle of the document. |
| type_middle | Insert "a", then Backspace, in the middle of the document. |
| type_end | The same at the end of the document. |
| undo_redo | Undo, then Redo, of a paste at the end. |

`--accessibility` adds an `AccessKitTree::update` after each frame, as the host
does while an assistive technology is active.

## Results

Microseconds per operation. "One buffer" is the same benchmark with the whole
text area in one Astrelis buffer, before paragraph chunks (one run).

| Case | Lines | One buffer | Current | Current with accessibility |
| --- | ---: | ---: | ---: | ---: |
| caret_vertical | 100 | 0.5 | 0.6 | 2.4–2.5 |
| type_middle | 100 | 570.5 | 67.6–73.4 | 72.0–73.1 |
| type_end | 100 | 558.9 | 31.1–35.6 | 34.8–35.4 |
| undo_redo | 100 | 558.6 | 31.4–34.9 | 35.1–35.7 |
| caret_vertical | 1,000 | 0.6 | 0.6–0.7 | 10.7–10.9 |
| type_middle | 1,000 | 5,928 | 92.8–99.7 | 115.8–116.7 |
| type_end | 1,000 | 5,940 | 34.7–39.4 | 56.0–56.4 |
| undo_redo | 1,000 | 6,029 | 36.5–42.3 | 57.1–58.0 |
| caret_vertical | 5,000 | 0.6 | 0.6–0.7 | 44.5–45.1 |
| type_middle | 5,000 | 31,517 | 172.2–180.0 | 264.8–276.7 |
| type_end | 5,000 | 31,778 | 137.9–144.2 | 236.9–261.8 |
| undo_redo | 5,000 | 32,757 | 145.1–150.6 | 241.8–292.9 |

Idle redraws take 0.2 µs at every size. Before AccessKit text runs were reused per
paragraph, typing with accessibility active took 4.7 ms at 5,000 lines and 0.93 ms
at 1,000 ([raw](text/text-accessibility-whole-runs.csv)).

## Where the time went

With one buffer, sampling `type_middle` at 5,000 lines put almost all of each
keystroke in Astrelis whole-document work: rebuilding the interaction index (about
38%), preparing glyph geometry (about 37%, half of it SipHash in the glyph cache)
and building the layout snapshot (about 17%). Astrelis reshapes only changed
paragraphs, but the snapshot, the index and the prepared geometry all cover the
whole buffer. The changes, in order:

| Change | type_middle at 5,000 lines |
| --- | --- |
| Paragraph chunks: `UiPainter` lays out, prepares and draws long wrapped text in chunks of 64 paragraphs, re-chunking only the region an edit touched | 31.5 → 0.61 ms |
| Byte-block control-character validation (it was a quarter of the remainder) and early-exit paragraph counting | 0.61 → 0.49 ms |
| Chunks of 8 paragraphs | 0.49 → 0.18 ms |
| AccessKit text runs reuse unchanged paragraphs' runs, identities and encodings | with accessibility, 4.7 → 0.27 ms |

Chunk size was tuned on this benchmark: 64, 32, 16 and 8 paragraphs took 489, 296,
207 and 178 µs for type_middle at 5,000 lines, and 564, 210, 117 and 68 µs at 100
lines. Text with at least 16 paragraphs is chunked; below 4 it returns to one
buffer. A GPU test checks that chunked carets, neighbors (including bidirectional
paragraph crossings), hit tests, selection rectangles and sizes match a single
buffer through edits.

## Effect on existing benchmarks

The frame benchmark's single-line `type_character` and the other cases were run
against `db84b44`, alternating base and current binaries run by run (four rounds;
two with accessibility). Microseconds, ranges of medians:

| Case | Rows | Base | Current |
| --- | ---: | ---: | ---: |
| idle_encode | 100 | 13.34–13.38 | 13.54–13.71 |
| type_character | 100 | 13.60–13.73 | 13.52–13.68 |
| opacity_animation | 100 | 6.70–6.76 | 6.80–6.83 |
| idle_encode | 1,000 | 28.48–28.64 | 29.55–29.95 |
| wheel_scroll | 1,000 | 32.55–32.61 | 32.49–34.44 |
| type_character | 1,000 | 38.95–39.61 | 38.42–39.84 |
| theme_switch | 1,000 | 93.57–94.43 | 92.19–92.90 |
| opacity_animation | 1,000 | 33.35–35.47 | 34.04–35.26 |

Other cases are within 2%. `idle_encode` is about 1 µs (4%) slower at 1,000 rows;
the cause was not isolated. Two hot-path costs this branch first introduced were
removed while measuring: the text pass checked new chunk fields before the existing
prepared-text fast path, and a new `options` field grew every `ElementKind` from 64
to 72 bytes (text inputs now keep their submit listener in the boxed options, and
the kind is 56 bytes). With accessibility, wheel and typing at 1,000 rows are
unchanged (292–293 µs and 297–301 µs).

## Remaining costs and limits

- Every edit still copies and compares the whole value several times: the
  controlled proposal, the application's accepted copy, the next description,
  reconciliation, history and chunk diffing. At 305 KB that is the remaining
  0.1–0.2 ms; it grows linearly with document size.
- Chunks are whole paragraphs. A single paragraph of hundreds of kilobytes, such
  as minified text without line breaks, still lays out and prepares as one unit.
- The first layout, a width change or a font change lays out every chunk.
- Glyph-cache hashing in Astrelis uses SipHash; switching it to a faster hasher is
  an Astrelis change.
- With accessibility active, each edit republishes the parent node and its list of
  paragraph runs, and a selection change rebuilds that list (45 µs at 5,000 lines).
  Runs are per paragraph, not per visual line, and carry no character geometry.
