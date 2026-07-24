#!/usr/bin/env bash
set -euo pipefail

version="0.1.0-rc.1"
astrelis_version="0.3.0-rc.1"
mode="${1:-package}"
registry_probe_dir="/tmp/rxui-release-registry-probe"

layers=(
  "rxui-core"
  "rxui-controls rxui-workbench rxui-native"
  "rxui"
  "rxui-testing"
)

required_astrelis=(
  astrelis-app astrelis-compositor astrelis-core astrelis-gpu
  astrelis-paint astrelis-paint-gpu astrelis-platform
  astrelis-platform-winit astrelis-text astrelis-ui-core
  astrelis-ui-host astrelis-ui-next astrelis-ui-testing
)

usage() {
  echo "usage: $0 [package|self-test|status|publish]" >&2
  exit 2
}

registry_has() {
  mkdir -p "$registry_probe_dir"
  (
    cd "$registry_probe_dir"
    cargo info --registry crates-io "$1@$2" >/dev/null 2>&1
  )
}

visible() {
  registry_has "$1" "$version"
}

wait_until_visible() {
  local package="$1"
  local attempt
  for attempt in {1..40}; do
    if visible "$package"; then
      echo "$package@$version is visible"
      return 0
    fi
    echo "waiting for $package@$version to reach the registry index ($attempt/40)"
    sleep 15
  done
  echo "$package@$version was uploaded but is not visible yet; rerun later" >&2
  return 1
}

check_astrelis() {
  local package
  for package in "${required_astrelis[@]}"; do
    if ! registry_has "$package" "$astrelis_version"; then
      echo "required $package@$astrelis_version is not visible on crates.io" >&2
      return 1
    fi
  done
}

case "$mode" in
  package)
    cargo package --workspace --allow-dirty --no-verify
    ;;
  self-test)
    registry_has rxui 0.0.0
    if registry_has rxui 0.1.0-rc.999999; then
      echo "registry probe incorrectly accepted a nonexistent version" >&2
      exit 1
    fi
    echo "registry exact-version probe passed"
    ;;
  status)
    for layer in "${layers[@]}"; do
      for package in $layer; do
        if visible "$package"; then
          echo "published $package@$version"
        else
          echo "pending   $package@$version"
        fi
      done
    done
    ;;
  publish)
    check_astrelis
    layer_number=0
    for layer in "${layers[@]}"; do
      layer_number=$((layer_number + 1))
      if [[ -t 0 ]]; then
        read -r -p "Publish layer $layer_number: $layer? [y/N] " answer
        [[ "$answer" == "y" || "$answer" == "Y" ]] || exit 0
      else
        echo "publish requires an interactive terminal for layer confirmation" >&2
        exit 2
      fi
      for package in $layer; do
        if visible "$package"; then
          echo "skipping existing $package@$version"
          continue
        fi
        cargo publish --package "$package"
        wait_until_visible "$package"
      done
    done
    ;;
  *) usage ;;
esac
