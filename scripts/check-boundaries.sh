#!/usr/bin/env bash
set -euo pipefail

manifest="crates/rxui-tree/Cargo.toml"
allowed=" astrelis-core astrelis-paint astrelis-platform astrelis-text "
found_disallowed=0

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

if (( found_disallowed )); then
  exit 1
fi

printf 'Boundary check OK: rxui-tree uses only allowed astrelis crates.\n'
