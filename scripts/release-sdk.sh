#!/bin/sh
# Package the public Rust SDK, or dry-run/publish it in dependency order.
#
# Usage: scripts/release-sdk.sh [--check|--dry-run|--publish]
#
#   --check    package every crate and print its file list (default)
#   --dry-run  cargo publish --dry-run for every crate; the dependencies must
#              already be published for the dependent crates
#   --publish  cargo publish in order (error -> bundle -> guest -> service ->
#              client -> cargo-anth)
#
# The crates.io token is read from the standard cargo credentials; in this
# environment they live under /tools/cargo.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"
mode=check
case "${1:-}" in
    --check) mode=check ;;
    --dry-run) mode=dry ;;
    --publish) mode=publish ;;
    -h | --help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

cargo_bin=${CARGO:-cargo}
if ! command -v "$cargo_bin" >/dev/null 2>&1; then
    cargo_bin=/tools/cargo/bin/cargo
fi

# Dependency order: error and bundle first, then the libraries and the CLI.
crates="anth-error anth-bundle anth-guest anth-service anth-client cargo-anth"
for crate in $crates; do
    case "$crate" in
        anth-error | anth-bundle) manifest=engine/Cargo.toml ;;
        *) manifest=sdk/rust/Cargo.toml ;;
    esac
    echo "sdk: $mode $crate"
    case "$mode" in
        check)
            "$cargo_bin" package --list --allow-dirty --manifest-path "$manifest" -p "$crate" >/dev/null
            ;;
        dry)
            "$cargo_bin" publish --dry-run --allow-dirty --manifest-path "$manifest" -p "$crate"
            ;;
        publish)
            "$cargo_bin" package --list --manifest-path "$manifest" -p "$crate" >/dev/null
            "$cargo_bin" publish --manifest-path "$manifest" -p "$crate"
            ;;
    esac
done
echo "sdk: $mode complete: $crates"
