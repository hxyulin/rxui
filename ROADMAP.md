# RXUI roadmap

## Product boundary

RXUI is an idiomatic Rust, retained-mode, custom-rendered framework for
desktop tools and editors. It does not emulate Qt's object model or wrap native
OS controls. Mobile, game-first HUD tooling, and a declarative reconciler are
outside the first stable release. Modern WebGPU browsers remain supported.

Astrelis owns retained tree mechanics and engine primitives. General-purpose
missing hooks belong in Astrelis; application and editor policy stays in RXUI.

## Release gates

- **0.1.0-rc.1 — First public preview:** application hosting, commands,
  design-system widgets, native menus, shell conventions, persistence,
  docking, editor views, devtools, testing helpers, native/browser examples,
  and performance budgets.
- **0.1 stable:** public API review, documented SemVer policy, clean consumer
  builds from crates.io, and validated Windows/macOS/Linux release examples.
- **1.0 stable desktop:** native screen-reader adapters backed by the semantic
  tree, mature application/window conventions, and a supported compatibility
  policy.

## Major missing capabilities

- Native accessibility adapters; semantic trees and actions already exist,
  but operating-system assistive technologies cannot consume them yet.
- Multiline and rich/code text editing with line navigation, large-document
  virtualization, syntax spans, and undo integration.
- Filesystem watching (open/save dialogs, URL/file launching, recent
  documents, and external file drag and drop shipped in `rxui-services` and
  the platform event stream; watching remains).
- Browser-side file dialogs; `rxui-services` dialogs currently report
  unsupported on wasm.
- A supported image decoding/loading widget path above external GPU images.
- Charting for line, bar, and scatter plots with axes, legends, zooming, and
  large-data decimation.
- Node-graph editing with ports, routed edges, box selection, keyboard editing,
  pan/zoom, and serialization.
- Later hardening for localization and RTL layout, high contrast, reduced
  motion, and animation/transitions.

Every interactive feature must ship with keyboard behavior, focus handling,
semantic coverage, deterministic tests, and correct idle invalidation.
