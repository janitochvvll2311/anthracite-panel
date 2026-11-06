#!/bin/sh
# Package one app directory as a bundle: manifest, the app package's WIT files
# (not `wit/deps`, which the engine injects), the component and web assets.
#
# Usage: scripts/package-app.sh apps/hello [output.zip]
set -eu
app=${1:?usage: package-app.sh <app-dir> [output.zip]}
out=${2:-$(pwd)/$(basename "$app").zip}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cp "$app/manifest.toml" "$tmp/"
mkdir -p "$tmp/wit"
for file in "$app"/wit/*.wit; do
    [ -e "$file" ] || continue
    cp "$file" "$tmp/wit/"
done
[ -f "$app/wasm/app.wasm" ] || { echo "build the component first: scripts/build-app.sh $app" >&2; exit 2; }
mkdir -p "$tmp/wasm"
cp "$app/wasm/app.wasm" "$tmp/wasm/"
if [ -d "$app/web" ]; then
    cp -R "$app/web" "$tmp/"
fi

rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
