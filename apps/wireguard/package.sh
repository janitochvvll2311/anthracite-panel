#!/usr/bin/env bash
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$here"

cargo build --release --target wasm32-unknown-unknown \
  --manifest-path backend/Cargo.toml
wasm=backend/target/wasm32-unknown-unknown/release/anth_plugin_wireguard.wasm
[ -f "$wasm" ] || { echo "missing $wasm" >&2; exit 1; }

rm -rf dist
mkdir -p dist/wasm dist/web
cp "$wasm" dist/wasm/plugin.wasm
cp web/wireguard.js dist/web/wireguard.js
cp manifest.toml dist/manifest.toml
cp wasm/manifest.toml dist/wasm/manifest.toml
cp web/manifest.toml dist/web/manifest.toml
wasm_sha=$(sha256sum dist/wasm/plugin.wasm | cut -d' ' -f1)
asset_sha=$(sha256sum dist/web/wireguard.js | cut -d' ' -f1)

bun -e '
const [path, digest] = process.argv.slice(1)
const text = (await Bun.file(path).text()).replace(/sha256 = "0+"/, `sha256 = "${digest}"`)
await Bun.write(path, text)
' dist/wasm/manifest.toml "$wasm_sha"
bun -e '
const [path, digest] = process.argv.slice(1)
const text = (await Bun.file(path).text()).replace(/sha256 = "0+"/, `sha256 = "${digest}"`)
await Bun.write(path, text)
' dist/web/manifest.toml "$asset_sha"

bun ../sign.mjs dist dist/manifest.sig dist/wireguard-key.pem dist/wireguard-public-key.txt

(cd dist && zip -qr org.anth.wireguard.zip manifest.toml manifest.sig wasm web)
echo "wrote dist/org.anth.wireguard.zip"
echo "public key: $(cat dist/wireguard-public-key.txt)"
