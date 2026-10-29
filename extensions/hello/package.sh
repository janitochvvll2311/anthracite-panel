#!/usr/bin/env bash
# Build the hello bundle: wasm guest, UI module, manifest, signature and zip.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$here"

cargo build --release --target wasm32-unknown-unknown \
  --manifest-path backend/Cargo.toml
wasm=backend/target/wasm32-unknown-unknown/release/anth_plugin_hello.wasm
[ -f "$wasm" ] || { echo "missing $wasm" >&2; exit 1; }

mkdir -p dist/web
cp "$wasm" dist/extension.wasm
cp web/hello.js dist/web/hello.js
wasm_sha=$(sha256sum dist/extension.wasm | cut -d' ' -f1)
asset_sha=$(sha256sum dist/web/hello.js | cut -d' ' -f1)

bun -e '
const [src, dst, wasm, asset] = process.argv.slice(1)
let i = 0
const text = (await Bun.file(src).text()).replace(/sha256 = "0+"/g, () =>
  `sha256 = "${[wasm, asset][i++]}"`)
if (i !== 2) throw new Error(`expected 2 zero digests, replaced ${i}`)
await Bun.write(dst, text)
' extension.toml dist/extension.toml "$wasm_sha" "$asset_sha"

bun ../sign.mjs dist/extension.toml dist/extension.sig dist/hello-key.pem dist/hello-public-key.txt

rm -f dist/org.anth.hello.zip
(cd dist && zip -qr org.anth.hello.zip extension.toml extension.wasm extension.sig web)
echo "wrote dist/org.anth.hello.zip"
echo "public key: $(cat dist/hello-public-key.txt)"
