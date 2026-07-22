# RXUI

A batteries-included retained-mode UI framework for Rust desktop applications
and editors, powered by [Astrelis](https://github.com/hxyulin/astrelis).

RXUI is retained and message-driven; the name does not imply a ReactiveX
Observable programming model.

RXUI owns application conventions, commands, polished widgets, editor
compositions, and testing tools. Astrelis remains the lower-level engine for
windows, scheduling, text, layout, retained UI state, painting, and GPU
composition.

## Status

RXUI `0.1.0-rc.1` is the first public preview and includes:

- an idle-efficient native/browser UI window host with asynchronous WebGPU startup;
- a high-level application runner (`App`, `run`) with typed messages,
  multi-window hosting, timers, keyed latest-value posting, and thread-safe
  message proxies;
- composable feature-local message mapping with scoped application contexts;
- typed commands, shortcut routing, and application menu models;
- native global menus on macOS and per-window menus on Windows;
- a theme-aware vector icon API and essential built-in icon set;
- radio groups, combo boxes, numeric fields, form sections, and icon buttons;
- responsive command toolbars and retained modal dialogs;
- synchronous field/form validation and accessible actionable toasts;
- reusable fallible undo/redo actions with conventional command integration;
- atomic versioned JSON state and monitor-safe window placement restoration;
- message-driven native file dialogs, URL and path launching, and persisted
  recent-document tracking;
- portable byte-oriented browser file opening/downloads and debounced native
  filesystem watching;
- decoded PNG/JPEG/WebP images with contain, cover, fill, and pixel-size fitting;
- interactive line, bar, and scatter charts with axis-specific navigation,
  clamped viewports, live latest-data following, and deterministic large-data
  decimation;
- serializable node graphs with ports, routed edges, selection, keyboard
  editing, and pan/zoom;
- a coherent dockable editor workspace with backward-compatible named layouts;
- virtualized, accessible tree and sortable/resizable table views;
- typed text, number, boolean, and enum property inspection;
- a keyboard-first command palette over the shared command registry;
- texture-backed render views and an interactive reference 2D scene editor;
- façade crates for the Astrelis widget and docking foundations;
- deterministic semantic-action, model-level, and headless whole-application
  testing helpers;
- pinned Git dependencies with an optional sibling-repository override;
- an opt-in, read-only retained UI inspector with pointer picking;
- normalized semantic, layout, interaction, and display-list snapshots;
- native desktop smoke coverage and release-mode editor performance budgets;
- guided application, shell, and editor tutorials with migration notes.

Modern browsers with WebGPU are an officially supported target. RXUI uses
one supplied HTML canvas and keeps the same retained UI, compositor, and
application scheduling model on native and Web.

See [ROADMAP.md](ROADMAP.md) for release gates and scope.
Source users of the former Astreon name should follow the
[rename migration](docs/migrations/astreon-to-rxui.md).

## Run the examples

```sh
cargo run -p rxui --example hello
```

This opens a native window titled **RXUI hello**. Clicking **Greet** changes
the status line from “Ready” to “Welcome to RXUI!”. The desktop runtime
sleeps while the window is idle.

Explore the controls and themes:

```sh
cargo run -p rxui --example design_gallery
```

Explore hierarchical messages with two independent feature instances:

```sh
cargo run -p rxui --example message_architecture
```

Each counter also queues a 1,000-value preview burst to demonstrate that
instance-scoped `post_latest` delivery keeps only the final pending value.

Run the application shell:

```sh
cargo run -p rxui --example application_shell
```

Expect a native window with an overflowing command toolbar, an undoable value,
a validated Settings modal, actionable notifications, and window geometry that
is restored after closing and reopening the example.

Run the complete editor workflow:

```sh
cargo run -p rxui --example reference_editor
```

The reference editor synchronizes selection across its hierarchy, entity table,
rendered 2D scene, and property inspector. Drag or resize docked panels, pan and
zoom the scene, invoke commands from the palette, edit undoable properties, and
save or restore the named workspace layout.

Run the native/browser visual workflow showcase:

```sh
cargo run -p rxui --example workflow_studio
```

Workflow Studio combines an editable node graph, horizontally scrollable live
chart with a fixed vertical range, decoded image preview, portable graph/image
import, graph export, and native automatic reload when an imported file
changes. Use **Toggle live data** to append samples and **Follow latest** to
resume automatic scrolling after inspecting history.

Inspect a live retained tree (press F12, Command-Option-I, or use the launcher):

```sh
cargo run -p rxui --example devtools_inspector --features devtools
```

The inspector is read-only and excluded from default production builds.

The guided documentation starts at [Your first RXUI app](docs/tutorials/first-app.md),
then covers [feature-local message architecture](docs/tutorials/message-architecture.md),
the [application shell](docs/tutorials/application-shell.md), and the
[editor workspace](docs/tutorials/editor-workspace.md).

On macOS or Windows, run the native File/Edit/View/Window menu example:

```sh
cargo run -p rxui --example native_menu
```

The native-menu API remains available on Linux and Web for portable source
code, but installation returns `NativeMenuError::UnsupportedPlatform`; RXUI
does not render a fake native top bar on those targets.

### Robotic-arm editor

The shared native/browser demo contains a procedural 5-DoF arm, compositor-backed
3D viewport, accessible one-degree joint sliders, forward-kinematics telemetry,
and resizable control/telemetry panes. It redraws only after joint, camera, or UI
changes, so the application sleeps while idle.

Run it natively:

```sh
cargo run -p rxui --example robot_arm
```

Build the WebGPU version and generate its no-bundler JavaScript package:

```sh
scripts/build-web-demo.sh
python3 -m http.server --directory crates/rxui/web 8000
```

The script installs the wasm target and the pinned `wasm-bindgen-cli` if
missing, then builds the example and runs `wasm-bindgen`; see the script for
the underlying commands.

Build Workflow Studio for the browser with:

```sh
scripts/build-web-demo.sh workflow_studio
```

Then open `http://localhost:8000/workflow_studio.html` from the same server.

Then open `http://localhost:8000/robot_arm.html`. The generated package is
`crates/rxui/web/pkg`. Startup looks up `#rxui-canvas`, starts the browser
event loop on that canvas, and completes adapter/device creation asynchronously;
the host reports `HostStatus::Initializing` until WebGPU is ready.

Web intentionally reports native menus, filesystem-backed JSON state,
filesystem watching, folder selection, and permission-gated clipboard
operations as unavailable. File-content opening and byte downloads are
supported. Browser support assumes a modern WebGPU implementation and a secure
context (localhost is accepted).

## Development

For joint RXUI/Astrelis development:

```sh
cp .cargo/config.toml.example .cargo/config.toml
cargo test --workspace
```

Without the local configuration Cargo uses the reviewed Astrelis Git revision
recorded in the workspace manifest.

## License

Licensed under the MIT license. See [LICENSE-MIT](LICENSE-MIT).
