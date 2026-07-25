# Releasing RXUI

RXUI uses one synchronized version across its seven-package graph. The first
public candidate is `0.1.0-rc.1` and requires Astrelis `=0.3.0-rc.1`.

## Publishing is currently blocked

Two upstream conditions must be met before any RXUI package can be published,
and neither is satisfied today:

1. **`astrelis-ui-next` sets `publish = false`.** `rxui-core` depends on it
   non-optionally and re-exports it as `rxui::core`, so `cargo publish -p
   rxui-core` cannot succeed. `astrelis-ui-host` inherits the same block, which
   extends it to `rxui-native` and the `rxui` facade.
2. **Astrelis is consumed as a pinned git revision, not a registry version.**
   `cargo publish` rejects git dependencies. See `[workspace.dependencies]` in
   `Cargo.toml`; swap the `git`/`rev` pairs for plain `version` requirements once
   Astrelis publishes `0.3.0-rc.1` to crates.io.

Until both are resolved, treat the steps below as the intended flow rather than a
working one, and do not tag a release.

## Preparation

Run the full validation suite from a clean release commit. This mirrors the
`test`, `boundaries`, `hygiene`, and `msrv` jobs in `.github/workflows/ci.yml`:

```sh
cargo fmt --all --check
cargo test --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo check --workspace --all-features --target wasm32-unknown-unknown
cargo build -p rxui --all-features --examples
cargo run -p rxui --example native_smoke
./scripts/release-rxui.sh package
```

The package command prepares the coordinated workspace archives without
uploading them. Astrelis `0.3.0-rc.1` must be fully visible on crates.io before
publishing RXUI.

## Publishing

```sh
./scripts/release-rxui.sh self-test
./scripts/release-rxui.sh status
./scripts/release-rxui.sh publish
```

The script checks exact versions from a neutral directory, verifies the full
Astrelis registry graph, publishes RXUI in dependency layers, and pauses for
confirmation between layers. An upload error stops immediately and is never
retried automatically; rerun the command after rate limits or registry
propagation delays.

After all seven packages are visible, test a fresh crates.io-only consumer. Then
tag the published commit as `v0.1.0-rc.1` and create the matching GitHub
prerelease. Never tag before the complete registry graph succeeds.
