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
# The Lit example ships beside the slot module. It is not mounted from the
# manifest: it exists so the same bundle demonstrates both render paths, and a
# panel that mounts it manually gets a stateful component that survives reloads.
cp frontend/dist/hello-lit.js "$staging/web/hello-lit.js"
sed "s/^sha256 = \".*\"/sha256 = \"$digest\"/" plugin.toml > "$staging/plugin.toml"

mkdir -p dist
(cd "$staging" && zip -qr "$here/dist/$id.zip" .)
echo "wrote dist/$id.zip (sha256 $digest)"

# Unpacked into `plugins/bundles/<id>`, which is a registry root: the bundle
# takes its id from the directory holding its manifest, and only an id can hold
# a dot. Not beside the source, because `plugins/` holds sources: a registry
# over it registers each source directory too, and a source directory whose name
# equals another bundle's alias shadows it — `resolve` matches an id before an
# alias, and does not care that the id is disabled.
#
# `plugin.toml` here keeps its zero digest on purpose, so the source tree can
# never be mistaken for a bundle that loads.
bundles="$here/../bundles"
bundle="$bundles/$id"
rm -rf "$bundle"
mkdir -p "$bundle"
unzip -q "$here/dist/$id.zip" -d "$bundle"
echo "unpacked into $bundle"
echo "run with ANTH_PLUGINS=$(cd "$bundles" && pwd)"
