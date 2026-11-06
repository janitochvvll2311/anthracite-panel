#!/bin/sh
# Build one app bundle's component: compile for wasm32-unknown-unknown, embed the
# app WIT, and wrap it into a component with wasm-tools (§7.3, §11).
#
# Usage: scripts/build-app.sh apps/hello
set -eu

app=${1:?usage: build-app.sh <app-dir>}
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
case "$app" in
    /*) ;;
    *) app=$root/$app ;;
esac
cd "$root"
manifest="$app/manifest.toml"
cargo_toml="$app/Cargo.toml"
[ -f "$cargo_toml" ] || { echo "no $cargo_toml" >&2; exit 2; }
[ -f "$manifest" ] || { echo "no $manifest" >&2; exit 2; }

name=$(awk -F'"' '/^name[[:space:]]*=/ {print $2; exit}' "$cargo_toml")
[ -n "$name" ] || { echo "no package name in $cargo_toml" >&2; exit 2; }
wasm_path=$(awk -F'"' '/^path[[:space:]]*=/ {print $2; exit}' "$manifest")
[ -n "$wasm_path" ] || wasm_path=wasm/app.wasm

cargo_bin=${CARGO:-cargo}
if ! command -v "$cargo_bin" >/dev/null 2>&1; then
    if [ -x /tools/cargo/bin/cargo ]; then
        cargo_bin=/tools/cargo/bin/cargo
    fi
fi
"$cargo_bin" build --manifest-path apps/Cargo.toml --release --target wasm32-unknown-unknown -p "$name"
module="target/wasm32-unknown-unknown/release/$(echo "$name" | tr '-' '_').wasm"
[ -f "$module" ] || { echo "cargo did not produce $module" >&2; exit 2; }

wasm_tools=
for candidate in "${WASM_TOOLS:-}" wasm-tools "${CARGO_HOME:-}/bin/wasm-tools" "${HOME:-}/.cargo/bin/wasm-tools" /tools/cargo/bin/wasm-tools; do
    [ -n "$candidate" ] || continue
    if command -v "$candidate" >/dev/null 2>&1; then
        candidate=$(command -v "$candidate")
    fi
    if [ -x "$candidate" ]; then
        wasm_tools=$candidate
        break
    fi
done
[ -n "$wasm_tools" ] || { echo "wasm-tools not found; set WASM_TOOLS" >&2; exit 2; }

tmp=$(mktemp -d /tmp/anthracite-panel/app-build.XXXXXX 2>/dev/null || mktemp -d)
trap 'rm -rf "$tmp"' EXIT
"$wasm_tools" component embed --world app "$app/wit" "$module" -o "$tmp/embedded.wasm"
mkdir -p "$(dirname "$app/$wasm_path")"
"$wasm_tools" component new "$tmp/embedded.wasm" -o "$app/$wasm_path"

sha=$(sha256sum "$app/$wasm_path" | cut -d' ' -f1)
echo "built $app/$wasm_path"
echo "manifest sha256 = \"$sha\""
