# RXUI

RXUI is a typed, retained desktop UI framework for Rust, powered by
[Astrelis](https://github.com/hxyulin/astrelis).

Application state lives in ordinary Rust components. Components reduce typed
actions and return lightweight views; RXUI reconciles those views into an
incremental retained tree.

```rust
use rxui::{Component, ComponentContext, Theme, View, button, column, label};

struct Counter(i32);

enum Action {
    Increment,
}

impl Component for Counter {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, _cx: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Increment => self.0 += 1,
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        column((
            label(format!("Count: {}", self.0)).key("value"),
            button("Increment", Action::Increment).key("increment"),
        ))
    }
}
```

The public API includes:

- typed state-owning `Component` and `ComponentWithProps` reducers;
- tuple children for fixed structure and keyed collections for dynamic lists;
- controlled text fields, checkboxes, sliders, split panes, and buttons;
- semantic theme tokens and typed style builders;
- reusable forms, validation, dialogs, toasts, toolbars, and command palettes;
- retained chart, node-graph, image, render-view, and docking surfaces;
- host-executed clipboard and background-task requests with typed completion;
- headless `ComponentHost` and native `ComponentWindow` runtimes;
- deterministic interaction, semantic, layout, and differential golden tests.

The low-level Astrelis retained core is available through `rxui::core` for
specialized elements. The former message-application and mutable widget-tree
APIs have been removed; `rxui::*` and `rxui::prelude::*` expose only the
component API.

Internally, the implementation is split by responsibility:

- `rxui-core` owns components, reconciliation, styles, icons, and services;
- `rxui-controls` owns composite controls and validation;
- `rxui-workbench` owns charts, node graphs, docking, and editor surfaces;
- `rxui-native` owns native window/runtime integration;
- `rxui` is the stable aggregate facade application code should depend on.

## Examples

Run the headless examples:

```sh
cargo run -p rxui --example counter
cargo run -p rxui --example keyed_collection
cargo run -p rxui --example settings_form
cargo run -p rxui --example component_services
cargo run -p rxui --example editor_vertical_slice
```

Run the native examples:

```sh
cargo run -p rxui --example native_counter
cargo run -p rxui --example native_settings
cargo run -p rxui --example native_workbench
```

`native_workbench` exercises the broadest slice: docking, splitters, charts,
node graphs, overlays, forms, toolbar controls, hover, keyboard focus, and
native rendering.

## Development

The migration branch currently requires Astrelis checked out beside RXUI:

```sh
cargo test --workspace --all-targets
```

Layout and behavior parity tests are documented in
[UI conformance goldens](docs/ui-conformance-goldens.md).

## License

Licensed under the MIT license. See [LICENSE-MIT](LICENSE-MIT).
