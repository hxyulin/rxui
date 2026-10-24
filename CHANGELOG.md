# Changelog

RXUI follows [Semantic Versioning](https://semver.org). Below 1.0, a minor
version may contain breaking changes. The independently versioned
[draglet](crates/draglet/README.md) crate keeps its own version.

## [0.1.0] - 2026-10-24

First release.

### Core
- Persistent typed state with `Entity<T>`, weak listeners, dependency tracking
  and explicit flush boundaries.
- Declarative element builders, keyed reconciliation, components and
  per-window placements.
- Taffy layout: flex rows and columns, stacks, absolute overlays, scroll areas,
  split panes, and fixed- and variable-height virtual lists and trees.
- Pointer and keyboard routing, capture, focus scopes, Tab traversal and typed
  commands with shortcuts.
- Scoped local and background tasks that finish into live state.

### Controls
- Buttons (default, primary, quiet and danger), controlled text inputs and text
  areas with IME, undo, password and placeholder support, and rich text with links.
- Check boxes, switches, radio groups, sliders, progress indicators, selects
  and tooltips.
- Tabs, menus, popovers, modal dialogs, controlled docking with optional native
  cross-window dragging, and virtualized trees.

### Styling
- Dark, light and high-contrast themes with semantic color tokens, elevation
  shadows, text roles, a radius scale and balanced or compact density.
- `Theme::generate` builds a full palette from an accent and a neutral tint;
  system accents keep readable accent text.
- Implicit state and theme transitions, overlay motion, springs and reduced motion.

### Rendering and platform
- RXUI-owned wgpu renderer (`rxui-draw`) that draws into application-supplied
  devices, encoders and targets, with images, live framebuffers, group opacity
  and custom wgpu passes.
- Optional desktop host (`rxui-host`) for macOS, Windows and Linux: multiple
  windows, native menus (macOS and Windows), file dialogs, clipboard, file drop
  and saved window geometry.
- Accessibility through AccessKit, published only while assistive technology
  is active.

[0.1.0]: https://github.com/hxyulin/rxui/releases/tag/v0.1.0
