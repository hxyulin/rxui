# Changelog

All notable public RXUI changes are documented here.

## 0.1.0-rc.1 — Unreleased

The first public RXUI preview provides a retained, message-driven Rust UI
framework for desktop applications and editors, powered by Astrelis 0.3.

### Included

- Native and browser UI hosting with idle-efficient scheduling.
- Typed commands, shortcuts, native menus, undo/redo, state persistence, and
  application-shell conventions.
- Theme-aware widgets, forms, validation, dialogs, notifications, and command
  palettes.
- Docking, virtualized tree/table views, property editing, and render views.
- Retained UI inspection, deterministic semantic testing, native smoke tests,
  WebAssembly coverage, and editor performance budgets.

### Naming

- The project and crate family previously developed as Astreon are now RXUI.
- The framework remains retained and message-driven; “RXUI” does not imply a
  ReactiveX Observable API.
- See `docs/migrations/astreon-to-rxui.md` for mechanical source changes.

### Known gaps

- Native screen-reader adapters are not implemented yet.
- Multiline/rich text, platform file dialogs, image loading, charts, and node
  graphs remain future work.
