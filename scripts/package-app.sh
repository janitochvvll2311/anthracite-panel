#!/bin/sh
# Package one app directory as a bundle: manifest, the app package's WIT files
# (not `wit/deps`, which the engine injects), the component and web assets.
#
# Usage: scripts/package-app.sh apps/hello [output.zip]
#
# Signing is opt-in: set ANTHRACITE_SIGNING_KEY to the private key and `anth-sign`
# (or ANTH_SIGN to its path) signs the staging directory before it is zipped.
# Release artifacts are signed; development bundles are not.
set -eu
app=${1:?usage: package-app.sh <app-dir> [output.zip]}
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
case "$app" in
    /*) ;;
    *) app=$root/$app ;;
esac
case "${2:-}" in
    "") out=$(pwd)/$(basename "$app").zip ;;
    /*) out=$2 ;;
    *) out=$(pwd)/$2 ;;
esac
mkdir -p "$(dirname "$out")"
mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/app-pack.XXXXXX)
trap 'rm -rf "$tmp"' EXIT

cp "$app/manifest.toml" "$tmp/"
mkdir -p "$tmp/wit"
for file in "$app"/wit/*.wit; do
    [ -e "$file" ] || continue
    cp "$file" "$tmp/wit/"
done
wasm_path=$(awk -F'"' '/^\[wasm\]/{section=1;next} /^\[/{section=0} section && /^path[[:space:]]*=/ {print $2; exit}' "$app/manifest.toml")
[ -n "$wasm_path" ] || wasm_path=wasm/app.wasm
[ -f "$app/$wasm_path" ] || { echo "build the component first: scripts/build-app.sh $app" >&2; exit 2; }
mkdir -p "$tmp/$(dirname "$wasm_path")"
cp "$app/$wasm_path" "$tmp/$wasm_path"
if [ -d "$app/web" ]; then
    cp -R "$app/web" "$tmp/"
fi

if [ -n "${ANTHRACITE_SIGNING_KEY:-}" ]; then
    signer=${ANTH_SIGN:-anth-sign}
    if ! command -v "$signer" >/dev/null 2>&1; then
        echo "ANTHRACITE_SIGNING_KEY is set but \`$signer\` is not on PATH; build it with \`cargo build -p anth-signer\` or set ANTH_SIGN" >&2
        exit 2
    fi
    "$signer" "$tmp" --key "$ANTHRACITE_SIGNING_KEY"
else
    echo "unsigned: set ANTHRACITE_SIGNING_KEY to sign this bundle" >&2
fi

rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
