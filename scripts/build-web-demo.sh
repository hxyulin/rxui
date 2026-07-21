#!/usr/bin/env bash
# Builds the robot_arm WebGPU demo into crates/rxui/web/pkg for a no-bundler
# browser page. Requires the wasm32-unknown-unknown target and pins the
# wasm-bindgen CLI to the workspace's wasm-bindgen version.
set -euo pipefail
cd "$(dirname "$0")/.."

WASM_BINDGEN_VERSION="0.2.126"

rustup target list --installed | grep -q '^wasm32-unknown-unknown$' ||
  rustup target add wasm32-unknown-unknown

installed=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [[ "${installed}" != "${WASM_BINDGEN_VERSION}" ]]; then
  cargo install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked
fi

cargo build --release -p rxui --example robot_arm --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir crates/rxui/web/pkg --out-name robot_arm \
  target/wasm32-unknown-unknown/release/examples/robot_arm.wasm

echo "Built crates/rxui/web/pkg. Serve it with:"
echo "  python3 -m http.server --directory crates/rxui/web 8000"
echo "then open http://localhost:8000/robot_arm.html"
