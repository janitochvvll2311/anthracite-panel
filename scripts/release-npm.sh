#!/bin/sh
# Build, pack and optionally publish the @anth-panel npm packages.
#
# Usage: scripts/release-npm.sh [--check|--publish]
#
#   --check    build and `bun pm pack --dry-run` each package (default)
#   --publish  build and `bun publish --access public` each package
#
# Publication is intentionally manual until a CI runner exists. The npm token
# is read from /tools/anthracite-publish/env.sh by the release shell.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"
mode=check
case "${1:-}" in
    --check) mode=check ;;
    --publish) mode=publish ;;
    -h | --help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

bun_bin=${BUN:-bun}
if ! command -v "$bun_bin" >/dev/null 2>&1; then
    bun_bin=/tools/bun/bin/bun
fi

packages="panel client wit cli create-app bun-plugin"
for package in $packages; do
    dir="sdk/ts/$package"
    echo "npm: $mode $package"
    (
        cd "$dir"
        if [ -f bun.lock ] || [ "$package" = panel ] || [ "$package" = client ] || [ "$package" = bun-plugin ]; then
            "$bun_bin" install --frozen-lockfile 2>/dev/null || "$bun_bin" install
        fi
        if grep -q '"build"' package.json; then
            "$bun_bin" run build
        fi
        if [ "$mode" = publish ]; then
            "$bun_bin" publish --access public
        else
            "$bun_bin" pm pack --dry-run
        fi
    )
done
echo "npm: $mode complete: $packages"
if [ "$mode" = check ]; then
    git diff --exit-code -- sdk/ts/panel/dist sdk/ts/client/dist sdk/ts/bun-plugin/dist
fi
