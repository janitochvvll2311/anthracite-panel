#!/usr/bin/env bash
# Build one plugin bundle: the wasm module, the web assets and the manifest.
#
# Two independent projects feed one archive:
#   backend/   a Rust cdylib compiled to wasm32-unknown-unknown
#   frontend/  a Bun project built to plain ES modules
#
# The host re-hashes plugin.wasm on every start and compares it to the digest in
# the manifest, so the hash is written only after the file is final.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$here"

id=$(sed -n 's/^id = "\(.*\)"/\1/p' plugin.toml | head -1)
[ -n "$id" ] || { echo "plugin.toml has no [plugin] id" >&2; exit 1; }

echo "building backend for wasm32-unknown-unknown"
(cd backend && cargo build --release --target wasm32-unknown-unknown)
wasm=backend/target/wasm32-unknown-unknown/release/anth_plugin_hello.wasm
[ -f "$wasm" ] || { echo "missing $wasm" >&2; exit 1; }

echo "building frontend"
(cd frontend && bun install --frozen-lockfile && bun run build)

digest=$(sha256sum "$wasm" | cut -d' ' -f1)

echo "assembling $id"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/web"
cp "$wasm" "$staging/plugin.wasm"
cp frontend/dist/hello.js "$staging/web/hello.js"
sed "s/^sha256 = \".*\"/sha256 = \"$digest\"/" plugin.toml > "$staging/plugin.toml"

mkdir -p dist
(cd "$staging" && zip -qr "$here/dist/$id.zip" .)
echo "wrote dist/$id.zip (sha256 $digest)"
