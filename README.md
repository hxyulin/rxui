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
- deterministic interaction, semantic, and layout golden tests.

The Astrelis vocabulary RXUI's own signatures speak is re-exported closed, as
`rxui::{geometry, color, input, semantics, paint}` plus `rxui::engine` for
authoring a retained element of your own. Nothing in RXUI's public API forces a
direct dependency on an Astrelis crate.

## Crates and features

Four published packages, on one axis: **what an application cannot do without,
versus what it can.**

- `rxui-core` is everything unconditional - components, the view protocol,
  styles, icons, services, and every composition built from them. It has **no
  `[features]` table**, and CI asserts both that and that it never reaches wgpu,
  winit, taffy, or the deprecated retained engine.
- `rxui-widgets` holds every surface an application can drop, one feature each:
  `charts`, `graph`, `docking`, and the non-default `devtools`.
- `rxui-native` opens windows. Its default `winit` feature drives an event loop;
  turn it off to embed a `ComponentWindow` in a loop you already own.
- `rxui` is the facade application code depends on, with features `native`,
  `native-embedded`, `widgets`, `charts`, `docking`, `graph`, and `devtools`.

`native` is the only real dependency cliff in the repo, and turning it off is
measurable: `rxui` resolves 172 crates with default features and 79 with
`default-features = false`, which is the whole component model, headless, with no
wgpu, winit, taffy, or clipboard. The widget features buy public API surface and
compile time rather than fewer dependencies - inside this workspace `rxui-core`
and `rxui-widgets` have identical dependency closures.

One caveat stated plainly: **"no text shaping" is not a configuration RXUI
offers.** `astrelis-paint` depends on `astrelis-text` and therefore parley
unconditionally, so no feature removes it.

Features are for *applications*. A library should depend on `rxui-core` directly
rather than on `rxui` with narrowed features: Cargo unifies features across a
dependency graph, so a library that narrows them makes its own surface depend on
whatever its consumer chose. That policy is why `rxui-core` stays feature-free.

`rxui-test-support` is unpublished scaffolding for RXUI's own suites. It sits
below `rxui` in the graph, which is what lets integration tests share a headless
harness without a `testing` feature on the library.

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

RXUI pins Astrelis to an exact git revision, so a plain clone builds without any
setup. To develop against a sibling Astrelis checkout instead, copy the example
Cargo configuration, which patches every Astrelis crate to `../astrelis`:

```sh
git clone https://github.com/hxyulin/astrelis ../astrelis
cp .cargo/config.toml.example .cargo/config.toml
cargo test --workspace --all-features --all-targets
```

`.cargo/config.toml` is gitignored, so the patch stays local to your checkout. It
does make Cargo rewrite `Cargo.lock` to path sources; see
[CONTRIBUTING.md](CONTRIBUTING.md) before committing that file.

Layout and behavior conformance tests are documented in
[UI conformance goldens](docs/ui-conformance-goldens.md). `cargo run -p rxui
--example native_smoke` renders a few frames in a real window and exits, which is
the cross-platform check that the native path still works.

## License

Licensed under the MIT license. See [LICENSE-MIT](LICENSE-MIT).
