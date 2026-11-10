#!/bin/sh
# Build the Home app bundle: Rust backend, Bun/TypeScript frontend, WIT and manifest.
# Usage: apps/home/build.sh [--demo] [output.anth]
#
# --demo packages the deterministic fixture backend (`home-demo`) instead of the
# real host reader, so a demo instance never exposes the machine it runs on.
set -eu

app=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
root=$(CDPATH= cd -- "$app/../.." && pwd)
demo=0
out=
for argument in "$@"; do
    case "$argument" in
        --demo) demo=1 ;;
        -h | --help)
            echo "usage: $0 [--demo] [output.anth]"
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

if [ "$demo" -eq 1 ]; then
    "$cargo_bin" build --manifest-path "$app/backend/Cargo.toml" --release --bin home-demo
    binary="$root/target/release/home-demo"
else
    "$cargo_bin" build --manifest-path "$app/backend/Cargo.toml" --release --bin home-service
    binary="$root/target/release/home-service"
fi
[ -x "$binary" ] || { echo "cargo did not produce $binary" >&2; exit 2; }

module="$app/frontend/dist/home.js"
[ -f "$module" ] || { echo "bun did not produce $module" >&2; exit 2; }
sha=$(sha256sum "$binary" | cut -d' ' -f1)
module_sha=$(sha256sum "$module" | cut -d' ' -f1)

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/home-build.XXXXXX)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin" "$tmp/web" "$tmp/wit"
cp "$binary" "$tmp/bin/home-service"
cp "$module" "$tmp/web/home.js"
cp "$app/wit/home.wit" "$tmp/wit/home.wit"
sed \
    -e "s/{{sha256}}/$sha/g" \
    -e "s/{{module_sha256}}/$module_sha/g" \
    "$app/manifest.toml.in" > "$tmp/manifest.toml"

mkdir -p "$(dirname "$out")"
rm -f "$out"
(cd "$tmp" && zip -qr "$out" .)
echo "wrote $out"
