# Changelog

All notable public RXUI changes are documented here.

## 0.1.0-rc.1 — Unreleased

### Component API cutover

- `rxui::*` and `rxui::prelude::*` now expose the typed component/view API.
- Components own durable state, reduce typed actions, emit typed effects, and
  return lightweight reconciled views.
- Fixed child structure uses tuples; dynamic collections require stable keys.
- Controls are controlled by component state while retaining transient focus,
  selection, caret, IME, hover, and pointer-capture state.
- Typed theme roles and style builders replace the old mutable widget facade.
- Headless and native component hosts share the same reconciliation and input
  paths.
- Clipboard, background work, and undo history use host-executed component
  service requests.
- Charts, node graphs, docking workspaces, images, render views, forms,
  validation, dialogs, toasts, toolbars, and inspection have component-native
  APIs.
- Synthetic interaction traces and reviewed semantic-geometry goldens cover
  layout, hover, focus, text input, splitters, keyed reconciliation, overlays,
  and specialized retained surfaces.
- The former `rxui-next` monolith is split into acyclic `rxui-core`,
  `rxui-controls`, `rxui-widgets`, `rxui-charts`, and `rxui-native`
  implementation crates. Application code continues to depend on the
  aggregate `rxui` facade.

### Removed

- The transitional `rxui::next`, `rxui::legacy`, and `next-default` surfaces.
- The old message-application, mutable widget, editor facade, devtools facade,
  native-menu, and desktop-service packages.
- Examples and tutorials authored against those removed APIs.
- The `rxui-testing` crate. Its differential oracle against the old retained
  engine is replaced by exact component-side layout assertions, and its
  headless harness moved down into the unpublished `rxui-test-support`.

The old retained UI is no longer referenced by any RXUI crate. It still reaches
the dependency graph through `astrelis-ui-host`, which is an engine concern.

### Known gaps

- Native screen-reader adapters are not implemented yet.
- Multiline and rich/code text editing remain future work.
- Native menus, file dialogs, filesystem watching, persistence, and the former
  message/subscription runtime need component-native replacements before they
  return to the public framework.
