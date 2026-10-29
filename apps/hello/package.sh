#!/usr/bin/env bash
# Build the hello bundle: wasm guest, layer manifests, signature and zip.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$here"

cargo build --release --target wasm32-unknown-unknown \
  --manifest-path backend/Cargo.toml
wasm=backend/target/wasm32-unknown-unknown/release/anth_plugin_hello.wasm
[ -f "$wasm" ] || { echo "missing $wasm" >&2; exit 1; }

rm -rf dist
mkdir -p dist/wasm dist/web
cp "$wasm" dist/wasm/plugin.wasm
cp web/hello.js dist/web/hello.js
cp manifest.toml dist/manifest.toml
cp wasm/manifest.toml dist/wasm/manifest.toml
cp web/manifest.toml dist/web/manifest.toml
wasm_sha=$(sha256sum dist/wasm/plugin.wasm | cut -d' ' -f1)
asset_sha=$(sha256sum dist/web/hello.js | cut -d' ' -f1)

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

bun ../sign.mjs dist dist/manifest.sig dist/hello-key.pem dist/hello-public-key.txt

(cd dist && zip -qr org.anth.hello.zip manifest.toml manifest.sig wasm web)
echo "wrote dist/org.anth.hello.zip"
echo "public key: $(cat dist/hello-public-key.txt)"
