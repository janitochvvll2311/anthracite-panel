#!/bin/sh
# Package one app directory as a bundle: manifest, the app package's WIT files
# (not `wit/deps`, which the engine injects), the component and web assets.
#
# Usage: scripts/package-app.sh apps/hello [output.zip]
set -eu
app=${1:?usage: package-app.sh <app-dir> [output.zip]}
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
case "$app" in
    /*) ;;
    *) app=$root/$app ;;
esac
case "${2:-}" in
    "") out=$root/$(basename "$app").zip ;;
    /*) out=$2 ;;
    *) out=$(pwd)/$2 ;;
esac
mkdir -p "$(dirname "$out")"
tmp=$(mktemp -d /tmp/anthracite-panel/app-pack.XXXXXX 2>/dev/null || mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cp "$app/manifest.toml" "$tmp/"
mkdir -p "$tmp/wit"
for file in "$app"/wit/*.wit; do
    [ -e "$file" ] || continue
    cp "$file" "$tmp/wit/"
done
wasm_path=$(awk -F'"' '/^path[[:space:]]*=/ {print $2; exit}' "$app/manifest.toml")
[ -n "$wasm_path" ] || wasm_path=wasm/app.wasm
[ -f "$app/$wasm_path" ] || { echo "build the component first: scripts/build-app.sh $app" >&2; exit 2; }
mkdir -p "$tmp/$(dirname "$wasm_path")"
cp "$app/$wasm_path" "$tmp/$wasm_path"
if [ -d "$app/web" ]; then
    cp -R "$app/web" "$tmp/"
fi

rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
