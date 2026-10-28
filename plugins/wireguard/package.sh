#!/usr/bin/env bash
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$here"

cargo build --release --target wasm32-unknown-unknown \
  --manifest-path backend/Cargo.toml
wasm=backend/target/wasm32-unknown-unknown/release/anth_plugin_wireguard.wasm
[ -f "$wasm" ] || { echo "missing $wasm" >&2; exit 1; }

mkdir -p dist/web
cp "$wasm" dist/plugin.wasm
cp web/wireguard.js dist/web/wireguard.js
wasm_sha=$(sha256sum dist/plugin.wasm | cut -d' ' -f1)
asset_sha=$(sha256sum dist/web/wireguard.js | cut -d' ' -f1)

bun -e '
const [src, dst, wasm, asset] = process.argv.slice(1)
let i = 0
const text = (await Bun.file(src).text()).replace(/sha256 = "0+"/g, () =>
  `sha256 = "${[wasm, asset][i++]}"`)
if (i !== 2) throw new Error(`expected 2 zero digests, replaced ${i}`)
await Bun.write(dst, text)
' plugin.toml dist/plugin.toml "$wasm_sha" "$asset_sha"

bun ../sign.mjs dist/plugin.toml dist/plugin.sig dist/wireguard-key.pem dist/wireguard-public-key.txt

rm -f dist/org.anth.wireguard.zip
(cd dist && zip -qr org.anth.wireguard.zip plugin.toml plugin.wasm plugin.sig web)
echo "wrote dist/org.anth.wireguard.zip"
echo "public key: $(cat dist/wireguard-public-key.txt)"
