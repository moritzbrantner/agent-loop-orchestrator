#!/usr/bin/env bash
set -euo pipefail

mode="${1:-setup}"
case "$mode" in
  "setup"|"maintenance") ;;
  *) printf 'usage: %s [setup|maintenance]\n' "$0" >&2; exit 2 ;;
esac

root="$(git rev-parse --show-toplevel)"
config="$root/.repository-environment.toml"
[[ -f "$config" ]] || { printf 'missing environment-v1 config: %s\n' "$config" >&2; exit 2; }

run_privileged() {
  if command -v sudo >/dev/null 2>&1; then sudo "$@"; else "$@"; fi
}

if [[ "$mode" == "setup" ]] && command -v apt-get >/dev/null 2>&1; then
  mapfile -t apt_packages < <(python3 - "$config" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as handle:
    data = tomllib.load(handle)
for package in data.get("system", {}).get("apt", []):
    print(package)
PY
  )
  if (( ${#apt_packages[@]} )); then
    run_privileged apt-get update
    run_privileged apt-get install -y --no-install-recommends "${apt_packages[@]}"
  fi
  if command -v bwrap >/dev/null 2>&1; then
    run_privileged chmod u+s "$(command -v bwrap)"
  fi
fi

rust_toolchain="$(python3 - "$root/rust-toolchain.toml" <<'PY'
import pathlib, sys, tomllib
path = pathlib.Path(sys.argv[1])
if path.is_file():
    print(tomllib.loads(path.read_text()).get("toolchain", {}).get("channel", ""))
PY
)"
if [[ -n "$rust_toolchain" ]]; then
  [[ "$rust_toolchain" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { printf 'Rust toolchain must be exact, got %s\n' "$rust_toolchain" >&2; exit 2; }
  command -v rustup >/dev/null 2>&1 || { printf '%s\n' 'rustup is required by the trusted base environment.' >&2; exit 2; }
  rustup toolchain install "$rust_toolchain" --profile minimal --component rustfmt,clippy
fi

bun_version="$(python3 - "$root/web/package.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
if path.is_file():
    value = json.loads(path.read_text()).get("packageManager", "")
    print(value.removeprefix("bun@") if value.startswith("bun@") else "")
PY
)"
[[ "$bun_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || { printf 'web/package.json must pin Bun exactly\n' >&2; exit 2; }
command -v bun >/dev/null 2>&1 || { printf '%s\n' 'bun is required by the trusted base environment.' >&2; exit 2; }
[[ "$(bun --version)" == "$bun_version" ]] || { printf 'Bun preflight mismatch: expected %s, got %s\n' "$bun_version" "$(bun --version)" >&2; exit 1; }

mapfile -t environment_commands < <(python3 - "$config" "$mode" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as handle:
    data = tomllib.load(handle)
for command in data.get(sys.argv[2], {}).get("commands", []):
    print(command)
PY
)
for command in "${environment_commands[@]}"; do
  (cd "$root" && bash -lc "$command")
done

if [[ -n "$rust_toolchain" ]]; then
  observed_rust="$(cd "$root" && rustc --version | awk '{print $2}')"
  [[ "$observed_rust" == "$rust_toolchain" ]] || { printf 'Rust preflight mismatch: expected %s, got %s\n' "$rust_toolchain" "$observed_rust" >&2; exit 1; }
fi
