#!/usr/bin/env bash
# Builds an RXUI WebGPU demo into crates/rxui/web/pkg for a no-bundler browser
# page. Requires the wasm32-unknown-unknown target and pins the wasm-bindgen CLI
# to the workspace's wasm-bindgen version.
#
# NOTE: RXUI does not currently ship a wasm-capable example. `rxui-native`
# compiles out `ComponentApplication` and `run_component` on wasm32 and provides
# no other way to obtain an `AppContext`, so the native examples build for the
# web only through empty `main` stubs. This script is kept working and generic;
# pass the example name once a real web entry point exists.
set -euo pipefail
cd "$(dirname "$0")/.."

WASM_BINDGEN_VERSION="0.2.126"

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <example>" >&2
  echo "available examples:" >&2
  find crates/rxui/examples -maxdepth 1 -name '*.rs' -exec basename {} .rs \; \
    | sort | sed 's/^/  /' >&2
  exit 2
fi

example="$1"
if [[ ! -f "crates/rxui/examples/${example}.rs" ]]; then
  echo "error: no such example: crates/rxui/examples/${example}.rs" >&2
  exit 2
fi

rustup target list --installed | grep -q '^wasm32-unknown-unknown$' ||
  rustup target add wasm32-unknown-unknown

installed=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [[ "${installed}" != "${WASM_BINDGEN_VERSION}" ]]; then
  cargo install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked
fi

cargo build --release -p rxui --example "${example}" --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir crates/rxui/web/pkg --out-name "${example}" \
  "target/wasm32-unknown-unknown/release/examples/${example}.wasm"

echo "Built crates/rxui/web/pkg. Serve it with:"
echo "  python3 -m http.server --directory crates/rxui/web 8000"
echo "then open http://localhost:8000/${example}.html"
