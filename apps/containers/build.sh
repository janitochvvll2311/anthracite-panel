#!/bin/sh
# Build the native Containers bundle: the Docker-compatible service, the Bun/TS
# frontend, WIT and manifest.
# Usage: apps/containers/build.sh [output.anth]
# Set ANTH_CONTAINERS_RUNTIME to install a QA bundle against another socket.
set -eu

app=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
root=$(CDPATH= cd -- "$app/../.." && pwd)
out=${1:-"$app/dist/containers.anth"}
runtime=${ANTH_CONTAINERS_RUNTIME:-/var/run/docker.sock}
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

"$cargo_bin" build --manifest-path "$app/backend/Cargo.toml" --release --bin containers-service
binary="$root/target/release/containers-service"
[ -x "$binary" ] || { echo "cargo did not produce $binary" >&2; exit 2; }

module="$app/frontend/dist/containers.js"
modal="$app/frontend/dist/containers-modal.js"
[ -f "$module" ] || { echo "bun did not produce $module" >&2; exit 2; }
[ -f "$modal" ] || { echo "bun did not produce $modal" >&2; exit 2; }
sha=$(sha256sum "$binary" | cut -d' ' -f1)
module_sha=$(sha256sum "$module" | cut -d' ' -f1)
modal_sha=$(sha256sum "$modal" | cut -d' ' -f1)

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/containers-build.XXXXXX)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin" "$tmp/web" "$tmp/wit"
cp "$binary" "$tmp/bin/containers-service"
cp "$module" "$tmp/web/containers.js"
cp "$modal" "$tmp/web/containers-modal.js"
cp "$app/wit/containers.wit" "$tmp/wit/containers.wit"
sed \
    -e "s/{{sha256}}/$sha/g" \
    -e "s/{{module_sha256}}/$module_sha/g" \
    -e "s/{{modal_sha256}}/$modal_sha/g" \
    -e "s/{{app_name}}/Containers/g" \
    -e "s/{{app_summary}}/Docker-compatible containers: lifecycle, logs, stats and exec behind the approved runtime socket./g" \
    -e "s|{{service_path}}|bin/containers-service|g" \
    -e "s|{{service_class}}|admin|g" \
    -e "s|{{runtime}}|$runtime|g" \
    -e "s|{{service_argv}}|[\"--socket\", \"{socket}\", \"--data\", \"{data}\", \"--broker\", \"{broker}\", \"--runtime\", \"$runtime\"]|g" \
    "$app/manifest.toml.in" > "$tmp/manifest.toml"

mkdir -p "$(dirname "$out")"
rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
