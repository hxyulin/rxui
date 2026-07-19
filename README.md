# Astreon

A batteries-included retained-mode UI framework for Rust desktop applications
and editors, powered by [Astrelis](https://github.com/hxyulin/astrelis).

Astreon owns application conventions, commands, polished widgets, editor
compositions, and testing tools. Astrelis remains the lower-level engine for
windows, scheduling, text, layout, retained UI state, painting, and GPU
composition.

## Status

Astreon is an early `0.1` foundation. The current vertical slice provides:

- an idle-efficient native UI window host;
- a typed command and shortcut registry;
- façade crates for the Astrelis widget and docking foundations;
- deterministic model-level testing helpers;
- pinned Git dependencies with an optional sibling-repository override.

See [ROADMAP.md](ROADMAP.md) for release gates and scope.

## Run the example

```sh
cargo run -p astreon --example hello
```

This opens a native window titled **Astreon hello**. Clicking **Greet** changes
the status line from “Ready” to “Welcome to Astreon!”. The desktop runtime
sleeps while the window is idle.

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
