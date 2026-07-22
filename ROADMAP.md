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
- Browser folder selection; portable file-content opening and byte downloads
  are available, but browsers cannot expose native `PathBuf` values or native
  filesystem watchers.
- Later hardening for localization and RTL layout, high contrast, reduced
  motion, and animation/transitions.

Every interactive feature must ship with keyboard behavior, focus handling,
semantic coverage, deterministic tests, and correct idle invalidation.

## Added for 0.1.0-rc.1

- Debounced native filesystem watching and portable browser file-content
  open/save services.
- PNG/JPEG/WebP decoding and retained image presentation.
- Line, bar, and scatter charts with axes, compact legends, axis-specific
  pan/zoom, viewport clamping, live latest-X following, keyboard selection,
  and deterministic decimation.
- Versioned serializable node graphs with ports, routed edges, box selection,
  keyboard editing, connection gestures, and pan/zoom.
