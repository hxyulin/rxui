# Astreon roadmap

## Product boundary

Astreon is an idiomatic Rust, retained-mode, custom-rendered framework for
desktop tools and editors. It does not emulate Qt's object model or wrap native
OS controls. Web, mobile, game-first HUD tooling, and a declarative reconciler
are outside the first stable release.

Astrelis owns retained tree mechanics and engine primitives. General-purpose
missing hooks are contributed to Astrelis; application and editor policy stays
here.

## Release gates

- **0.1 — Foundation (complete):** workspace, reproducible dependency workflow, native
  window host, typed commands, façade crates, testing helpers, and examples.
- **0.2 — Design system (complete):** stable theme vocabulary, vector icons,
  essential editor forms and input controls, shortcut routing, a widget
  gallery, and native macOS/Windows application menus.
- **0.3 — Application shell (complete):** responsive command toolbars, retained
  modal dialogs, actionable notifications, form validation, reusable undo/redo,
  and persisted window state.
- **0.4 — Editor-ready alpha (complete):** coherent docking workspace, tree/table/property
  views, render views, command palette, saved layouts, and a reference editor.
- **0.5 — Hardening (complete):** opt-in retained UI inspector, deterministic
  structural snapshots, native smoke tests, performance budgets, tutorials,
  and migration documentation.
- **1.0 — Stable desktop:** reviewed API and semver policy, Windows/macOS/Linux
  validation, and native screen-reader adapters backed by semantic trees.

Every interactive feature must ship with keyboard behavior, focus handling,
semantic coverage, deterministic tests, and correct idle invalidation.
