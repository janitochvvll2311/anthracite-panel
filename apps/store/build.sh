#!/bin/sh
# Build the native Store bundle: the catalogue service, the Bun/TS frontend,
# WIT and manifest.
# Usage: apps/store/build.sh [output.anth]
set -eu

app=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
root=$(CDPATH= cd -- "$app/../.." && pwd)
out=${1:-"$app/dist/store.anth"}
cd "$root"

cargo_bin=${CARGO:-cargo}
if ! command -v "$cargo_bin" >/dev/null 2>&1; then
    if [ -x /tools/cargo/bin/cargo ]; then
        cargo_bin=/tools/cargo/bin/cargo
    fi
fi
bun_bin=${BUN:-bun}
if ! command -v "$bun_bin" >/dev/null 2>&1; then
    if [ -x /tools/bun/bin/bun ]; then
        bun_bin=/tools/bun/bin/bun
    fi
fi

(
    cd "$app/frontend"
    "$bun_bin" install
    "$bun_bin" run typecheck
    if grep -q '"test"' package.json; then
        "$bun_bin" run test
    fi
    "$bun_bin" run build
)

"$cargo_bin" build --manifest-path "$app/backend/Cargo.toml" --release --bin store-service
binary="$root/target/release/store-service"
[ -x "$binary" ] || { echo "cargo did not produce $binary" >&2; exit 2; }

module="$app/frontend/dist/store.js"
modal="$app/frontend/dist/store-modal.js"
[ -f "$module" ] || { echo "bun did not produce $module" >&2; exit 2; }
[ -f "$modal" ] || { echo "bun did not produce $modal" >&2; exit 2; }
sha=$(sha256sum "$binary" | cut -d' ' -f1)
module_sha=$(sha256sum "$module" | cut -d' ' -f1)
modal_sha=$(sha256sum "$modal" | cut -d' ' -f1)

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/store-build.XXXXXX)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin" "$tmp/web" "$tmp/wit"
cp "$binary" "$tmp/bin/store-service"
cp "$module" "$tmp/web/store.js"
cp "$modal" "$tmp/web/store-modal.js"
cp "$app/wit/store.wit" "$tmp/wit/store.wit"
sed \
    -e "s/{{sha256}}/$sha/g" \
    -e "s/{{module_sha256}}/$module_sha/g" \
    -e "s/{{modal_sha256}}/$modal_sha/g" \
    -e "s/{{app_name}}/Store/g" \
    -e "s/{{app_summary}}/Signed catalogue browser and digest-pinned bundle cache; installation stays in the manager./g" \
    -e "s|{{service_path}}|bin/store-service|g" \
    -e "s/{{service_class}}/socket/g" \
    -e "s|{{service_argv}}|[\"--socket\", \"{socket}\", \"--data\", \"{data}\", \"--shared\", \"{shared}\", \"--broker\", \"{broker}\"]|g" \
    "$app/manifest.toml.in" > "$tmp/manifest.toml"

mkdir -p "$(dirname "$out")"
rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
