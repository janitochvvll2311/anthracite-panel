#!/bin/sh
# Build the native Terminal bundle: PTY service, Bun/TypeScript frontend, WIT
# and manifest. The demo instance keeps using the shared emulator bundle.
# Usage: apps/terminal/build.sh [output.anth]
set -eu

app=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
root=$(CDPATH= cd -- "$app/../.." && pwd)
out=${1:-"$app/dist/terminal.anth"}
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
    "$bun_bin" run build
)

"$cargo_bin" build --manifest-path "$app/backend/Cargo.toml" --release --bin terminal-service
binary="$root/target/release/terminal-service"
[ -x "$binary" ] || { echo "cargo did not produce $binary" >&2; exit 2; }

module="$app/frontend/dist/terminal.js"
[ -f "$module" ] || { echo "bun did not produce $module" >&2; exit 2; }
sha=$(sha256sum "$binary" | cut -d' ' -f1)
module_sha=$(sha256sum "$module" | cut -d' ' -f1)

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/terminal-build.XXXXXX)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin" "$tmp/web" "$tmp/wit"
cp "$binary" "$tmp/bin/terminal-service"
cp "$module" "$tmp/web/terminal.js"
cp "$app/wit/terminal.wit" "$tmp/wit/terminal.wit"
sed \
    -e "s/{{sha256}}/$sha/g" \
    -e "s/{{module_sha256}}/$module_sha/g" \
    -e "s/{{app_name}}/Terminal/g" \
    -e "s/{{app_summary}}/Interactive shell over a native PTY./g" \
    -e "s|{{service_path}}|bin/terminal-service|g" \
    -e "s/{{service_class}}/exec/g" \
    -e "s|{{service_argv}}|[\"--socket\", \"{socket}\"]|g" \
    "$app/manifest.toml.in" > "$tmp/manifest.toml"

mkdir -p "$(dirname "$out")"
rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
