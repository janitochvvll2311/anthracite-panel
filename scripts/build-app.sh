#!/bin/sh
# Build one app bundle's component: compile for wasm32-unknown-unknown, embed the
# app WIT, and wrap it into a component with wasm-tools (§7.3, §11).
#
# Usage: scripts/build-app.sh apps/hello
set -eu

app=${1:?usage: build-app.sh <app-dir>}
name=$(basename "$app")
manifest="$app/Cargo.toml"
[ -f "$manifest" ] || { echo "no $manifest" >&2; exit 2; }

cargo build --manifest-path apps/Cargo.toml --release --target wasm32-unknown-unknown -p "$name"
module="target/wasm32-unknown-unknown/release/$(echo "$name" | tr '-' '_').wasm"
wasm_tools=${WASM_TOOLS:-wasm-tools}
if ! command -v "$wasm_tools" >/dev/null 2>&1; then
    wasm_tools=/tools/cargo/bin/wasm-tools
fi
[ -x "$wasm_tools" ] || { echo "wasm-tools not found; set WASM_TOOLS" >&2; exit 2; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
"$wasm_tools" component embed --world app "$app/wit" "$module" -o "$tmp/embedded.wasm"
mkdir -p "$app/wasm"
"$wasm_tools" component new "$tmp/embedded.wasm" -o "$app/wasm/app.wasm"

sha=$(sha256sum "$app/wasm/app.wasm" | cut -d' ' -f1)
echo "built $app/wasm/app.wasm"
echo "manifest sha256 = \"$sha\""
