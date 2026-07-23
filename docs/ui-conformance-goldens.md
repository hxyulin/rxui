# UI conformance goldens

`rxui-testing` contains backend-independent synthetic scenes for catching
layout and interaction regressions without opening a native window.

## Suites

- `differential_golden` builds equivalent legacy and Next scenes, normalizes
  their labeled semantic geometry, and records both implementations plus
  per-landmark deltas.
- `next_conformance` covers Next-only primitives and deterministic interaction
  traces where no faithful legacy equivalent exists.

The initial catalog covers:

- fixed padding, gaps, and explicit control sizes;
- intrinsic form sizing and cross-implementation policy differences;
- horizontal split panes at 25%, 50%, and 75%;
- modal centering at 640x480 and 320x240;
- modal background disablement, autofocus, and Escape dismissal;
- exact 1:2:1 bounded flex growth;
- overlapping stack geometry and topmost hit routing;
- splitter click stability, pointer capture, outside release, and clamping;
- hover entry/window exit, cursor changes, and local repaint counts.

Run all conformance tests:

```sh
cargo test -p rxui-testing
```

Run only the focused suites with failure output:

```sh
cargo test -p rxui-testing --test differential_golden -- --nocapture
cargo test -p rxui-testing --test next_conformance -- --nocapture
```

## Updating goldens

Goldens never update during ordinary tests or CI. To export candidates after an
intentional layout change:

```sh
RXUI_UPDATE_GOLDENS=1 cargo test -p rxui-testing \
  --test differential_golden --test next_conformance
git diff -- crates/rxui-testing/tests/goldens
cargo test -p rxui-testing
```

Review the diff before committing. A changed delta is not automatically a bug:
for example, the legacy intrinsic form stretches controls to the available
width and reports unusually small text/button heights, while Next keeps
intrinsic widths and usable control heights. Fixed-size scenes that are meant
to be equivalent additionally use tolerance-based differential assertions, so
their golden cannot be updated to conceal a parity failure.

Add new scenes when fixing a visual or behavioral bug. Prefer a small scene
that isolates one contract, and include both:

1. a normalized golden for layout and semantic state; and
2. direct assertions for interaction state that cannot be represented by the
   semantic tree, such as cursor selection, capture lifetime, hit order, or
   repaint locality.
