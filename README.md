# Astreon

A batteries-included retained-mode UI framework for Rust desktop applications
and editors, powered by [Astrelis](https://github.com/hxyulin/astrelis).

Astreon owns application conventions, commands, polished widgets, editor
compositions, and testing tools. Astrelis remains the lower-level engine for
windows, scheduling, text, layout, retained UI state, painting, and GPU
composition.

## Status

Astreon `0.4` provides:

- an idle-efficient native UI window host;
- typed commands, shortcut routing, and application menu models;
- native global menus on macOS and per-window menus on Windows;
- a theme-aware vector icon API and essential built-in icon set;
- radio groups, combo boxes, numeric fields, form sections, and icon buttons;
- responsive command toolbars and retained modal dialogs;
- synchronous field/form validation and accessible actionable toasts;
- reusable fallible undo/redo actions with conventional command integration;
- atomic versioned JSON state and monitor-safe window placement restoration;
- a coherent dockable editor workspace with backward-compatible named layouts;
- virtualized, accessible tree and sortable/resizable table views;
- typed text, number, boolean, and enum property inspection;
- a keyboard-first command palette over the shared command registry;
- texture-backed render views and an interactive reference 2D scene editor;
- façade crates for the Astrelis widget and docking foundations;
- deterministic semantic-action and model-level testing helpers;
- pinned Git dependencies with an optional sibling-repository override.

See [ROADMAP.md](ROADMAP.md) for release gates and scope.

## Run the examples

```sh
cargo run -p astreon --example hello
```

This opens a native window titled **Astreon hello**. Clicking **Greet** changes
the status line from “Ready” to “Welcome to Astreon!”. The desktop runtime
sleeps while the window is idle.

Explore the 0.2 controls and themes:

```sh
cargo run -p astreon --example design_gallery
```

Run the complete 0.3 application shell:

```sh
cargo run -p astreon --example application_shell
```

Expect a native window with an overflowing command toolbar, an undoable value,
a validated Settings modal, actionable notifications, and window geometry that
is restored after closing and reopening the example.

Run the complete 0.4 editor workflow:

```sh
cargo run -p astreon --example reference_editor
```

The reference editor synchronizes selection across its hierarchy, entity table,
rendered 2D scene, and property inspector. Drag or resize docked panels, pan and
zoom the scene, invoke commands from the palette, edit undoable properties, and
save or restore the named workspace layout.

On macOS or Windows, run the native File/Edit/View/Window menu example:

```sh
cargo run -p astreon --example native_menu
```

The native-menu API remains available on Linux and Web for portable source
code, but installation returns `NativeMenuError::UnsupportedPlatform`; Astreon
does not render a fake native top bar on those targets.

## Development

For joint Astreon/Astrelis development:

```sh
cp .cargo/config.toml.example .cargo/config.toml
cargo test --workspace
```

Without the local configuration Cargo uses the reviewed Astrelis Git revision
recorded in the workspace manifest.

## License

Licensed under the MIT license. See [LICENSE-MIT](LICENSE-MIT).
