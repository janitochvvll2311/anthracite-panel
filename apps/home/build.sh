#!/bin/sh
# Build the Home app bundle: Rust backend, Bun/TypeScript frontend, WIT and manifest.
# Usage: apps/home/build.sh [output.anth]
#
# This packages the real host reader. The demo builds the same app from
# `demo/emulator`, so its fixture never reads the machine it runs on.
set -eu

app=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
root=$(CDPATH= cd -- "$app/../.." && pwd)
out=
for argument in "$@"; do
    case "$argument" in
        -h | --help)
            echo "usage: $0 [output.anth]"
            exit 0
            ;;
        *)
            if [ -n "$out" ]; then
                echo "unexpected argument: $argument" >&2
                exit 2
            fi
            out=$argument
            ;;
    esac
done
out=${out:-"$app/dist/home.anth"}
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

"$cargo_bin" build --manifest-path "$app/backend/Cargo.toml" --release --bin home-service
binary="$root/target/release/home-service"
[ -x "$binary" ] || { echo "cargo did not produce $binary" >&2; exit 2; }

module="$app/frontend/dist/home.js"
icon="$app/web/icon.svg"
[ -f "$module" ] || { echo "bun did not produce $module" >&2; exit 2; }
[ -f "$icon" ] || { echo "missing $icon" >&2; exit 2; }
sha=$(sha256sum "$binary" | cut -d' ' -f1)
module_sha=$(sha256sum "$module" | cut -d' ' -f1)
icon_sha=$(sha256sum "$icon" | cut -d' ' -f1)

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/home-build.XXXXXX)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin" "$tmp/web" "$tmp/wit"
cp "$binary" "$tmp/bin/home-service"
cp "$module" "$tmp/web/home.js"
cp "$icon" "$tmp/web/icon.svg"
cp "$app/wit/home.wit" "$tmp/wit/home.wit"
sed \
    -e "s/{{sha256}}/$sha/g" \
    -e "s/{{module_sha256}}/$module_sha/g" \
    -e "s/{{icon_sha256}}/$icon_sha/g" \
    -e "s|{{service_path}}|bin/home-service|g" \
    -e "s/{{service_class}}/host/g" \
    -e "s|{{service_argv}}|[\"--socket\", \"{socket}\"]|g" \
    "$app/manifest.toml.in" > "$tmp/manifest.toml"

mkdir -p "$(dirname "$out")"
rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
