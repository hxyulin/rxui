#!/usr/bin/env bash
set -euo pipefail

found_disallowed=0

check_manifest() {
  local manifest="$1"
  local allowed="$2"

  while IFS= read -r dependency; do
    case "$dependency" in
      astrelis-*)
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
  "crates/rxui-test/Cargo.toml" \
  " astrelis-core astrelis-platform astrelis-text "

if (( found_disallowed )); then
  exit 1
fi

printf 'Boundary check OK: rxui-tree and rxui-test use only allowed astrelis crates.\n'
