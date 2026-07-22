# Changelog

All notable public RXUI changes are documented here.

## 0.1.0-rc.1 — Unreleased

The first public RXUI preview provides a retained, message-driven Rust UI
framework for desktop applications and editors, powered by Astrelis 0.3.

### Included

- A high-level application runner: implement `rxui::prelude::App`
  (`build`/`update` plus optional hooks) and start with `rxui::app::run`; the
  runner owns window hosting, message routing, redraw scheduling, timers,
  cross-thread message proxies, and shutdown. Direct `astrelis_app::App`
  implementations remain supported.
- Hierarchical feature-message mapping, keyed latest-value queue coalescing,
  delayed timeout factories, and deterministic timer behavior shared by the
  native/browser runners and headless application harness.
- Runtime-neutral cancellable task completions and a configurable, bounded
  native blocking pool, with deterministic chosen-order task completion in
  the application harness, optional diagnostic names, and active-task runtime
  snapshots.
- Application-scoped declarative interval subscriptions with stable identity,
  latest-value delivery, lifecycle reconciliation, hierarchical mapping, and
  deterministic virtual-time testing.
- An opaque `rxui::Error`/`rxui::Result` pair that converts from any standard
  error, removing `map_err` glue from application code.
- Fluent `build(...)...finish()` constructors for `RadioGroup`, `ComboBox`,
  `NumericField`, and `Toolbar` alongside the existing fallible `new(...)`
  constructors.
- Desktop services: native file open/save dialogs, URL and file launching,
  and a persisted recent-documents list.
- Portable byte-oriented browser open/save dialogs and debounced native
  filesystem watching.
- PNG/JPEG/WebP image decoding plus retained image fitting and sampling.
- Interactive line, bar, and scatter charts with axes, legends, axis-specific
  pan/zoom, cursor-anchored native pinch and wheel zoom, two-axis precision
  scrolling, configurable input bindings, viewport clamping, live latest-X
  following, selection, and deterministic large-data decimation.
- Versioned serializable node-graph editing with ports, routed edges, box
  selection, connection gestures, keyboard editing, and shared gesture-aware
  pan/zoom navigation.
- Native and browser UI hosting with idle-efficient scheduling.
- Typed commands, shortcuts, native menus, undo/redo, state persistence, and
  application-shell conventions.
- Theme-aware widgets, forms, validation, dialogs, notifications, and command
  palettes.
- Docking, virtualized tree/table views, property editing, and render views.
- Retained UI and runtime inspection, deterministic semantic testing, native
  smoke tests, WebAssembly coverage, and editor performance budgets.

### Naming

- The project and crate family previously developed as Astreon are now RXUI.
- The framework remains retained and message-driven; “RXUI” does not imply a
  ReactiveX Observable API.
- See `docs/migrations/astreon-to-rxui.md` for mechanical source changes.

### Known gaps

- Native screen-reader adapters are not implemented yet.
- Multiline/rich text and browser folder selection remain future work; native
  accessibility adapters are also not implemented yet.
