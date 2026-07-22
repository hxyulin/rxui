# Releasing RXUI

RXUI uses one synchronized version for its eight-package graph. The first public
candidate is `0.1.0-rc.1` and requires Astrelis `=0.3.0-rc.1`.

## Preparation

Run the full validation suite from a clean release commit:

```sh
cargo fmt --all --check
cargo test --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo check --workspace --all-features --target wasm32-unknown-unknown
cargo run --release -p rxui --example visual_features_perf -- --check
./scripts/build-web-demo.sh workflow_studio
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

After all eight packages are visible, test a fresh crates.io-only consumer.
Then tag the published commit as `v0.1.0-rc.1` and create the matching GitHub
prerelease. Never tag before the complete registry graph succeeds.
