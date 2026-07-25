# UI conformance goldens

`crates/rxui/tests` contains backend-independent synthetic scenes for catching
layout and interaction regressions without opening a native window. They drive
`rxui_test_support::Harness`, an unpublished headless host that addresses the
retained tree by accessible label.

## Suites

- `layout_contracts` covers absolute padding, gap, and intrinsic-sizing
  contracts: the fixed row asserts exact offsets, and the intrinsic form pairs a
  reviewed sizing golden with usability policy assertions.
- `conformance` covers component primitives and deterministic interaction traces
  where the semantic tree alone cannot express the contract.

The catalog covers:

- fixed padding, gaps, and explicit control sizes at exact offsets;
- intrinsic form sizing, on-screen containment, and non-overlap;
- horizontal split panes at 25%, 50%, and 75%;
- modal centering at 640x480 and 320x240;
- modal background disablement, autofocus, and Escape dismissal;
- exact 1:2:1 bounded flex growth;
- overlapping stack geometry and topmost hit routing;
- splitter click stability, pointer capture, outside release, and clamping;
- hover entry/window exit, cursor changes, and local repaint counts.

Earlier revisions proved fixed-size layouts by diffing the component engine
against the old `astrelis-ui-core` engine within a 0.75 logical-pixel tolerance.
That oracle is retired: a compose-equivalence property test now lives in the
engine, and a fixed-size scene deserves the exact offsets `layout_contracts`
asserts rather than a tolerance band.

Run all conformance tests:

```sh
cargo test -p rxui --test layout_contracts --test conformance
```

Run one suite with failure output:

```sh
cargo test -p rxui --test conformance -- --nocapture
```

## Updating goldens

Goldens never update during ordinary tests or CI. To export candidates after an
intentional layout change:

```sh
RXUI_UPDATE_GOLDENS=1 cargo test -p rxui \
  --test layout_contracts --test conformance
git diff -- crates/rxui/tests/goldens
cargo test -p rxui --test layout_contracts --test conformance
```

Review the diff before committing. The fixed row additionally asserts its
geometry directly in Rust, so its offsets cannot be moved by blessing a golden.

Add new scenes when fixing a visual or behavioral bug. Prefer a small scene
that isolates one contract, and include both:

1. a normalized golden for layout and semantic state; and
2. direct assertions for interaction state that cannot be represented by the
   semantic tree, such as cursor selection, capture lifetime, hit order, or
   repaint locality.
