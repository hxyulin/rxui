#!/usr/bin/env bash
set -euo pipefail

found_disallowed=0

check_manifest() {
  local manifest="$1"
  local allowed="$2"

  [[ -f "$manifest" ]] || {
    echo "missing manifest: $manifest" >&2
    exit 1
  }

  while IFS= read -r dependency; do
    case "$dependency" in
      astrelis-*|rxui-*)
        if [[ "$allowed" != *" $dependency "* ]]; then
          printf 'Boundary check failed: %s depends on disallowed crate %s\n' \
            "$manifest" "$dependency" >&2
          found_disallowed=1
        fi
        ;;
    esac
  done < <(
    awk '
      /^\[/ { in_dependencies = ($0 ~ /dependencies\]$/); next }
      in_dependencies && /^[[:space:]]*[A-Za-z0-9_-]+[[:space:]]*=/ {
        key = $0
        sub(/[[:space:]]*=.*/, "", key)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", key)
        print key
      }
    ' "$manifest"
  )
}

check_manifest \
  "crates/rxui-tree/Cargo.toml" \
  " astrelis-core astrelis-paint astrelis-platform astrelis-text "
check_manifest \
  "crates/rxui-core/Cargo.toml" \
  " rxui-tree astrelis-core astrelis-paint astrelis-platform astrelis-text "
check_manifest \
  "crates/rxui-widgets/Cargo.toml" \
  " rxui-core rxui-tree astrelis-core astrelis-paint astrelis-platform astrelis-text "
check_manifest \
  "crates/rxui-test/Cargo.toml" \
  " rxui-core rxui-tree astrelis-core astrelis-platform astrelis-text "
check_manifest \
  "crates/rxui-host/Cargo.toml" \
  " rxui-core rxui-tree astrelis-app astrelis-compositor astrelis-core astrelis-gpu astrelis-gpu-wgpu astrelis-paint astrelis-paint-gpu astrelis-platform astrelis-platform-winit astrelis-text astrelis-text-gpu "

if (( found_disallowed )); then
  exit 1
fi

printf 'Boundary check OK: rxui-core, rxui-tree, rxui-widgets, rxui-test, and rxui-host use only allowed dependencies.\n'
