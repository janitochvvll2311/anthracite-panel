#!/bin/sh
# Publish the four release images to GHCR.
#
# Usage: scripts/release-ghcr.sh --images <dir> [--version <version>]
#                                 [--owner <github-owner>] [--public]
#
# <dir> is the directory release-images.sh writes:
# anthracite-<version>-<variant>.tar.gz.
#
# Authentication: `skopeo login ghcr.io` first (a token with write:packages).
# `--public` tries to flip both GHCR packages to public visibility with
# `gh api`. GitHub refuses that call for many token types (404), so the script
# prints the settings URL instead of failing after the images were pushed.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"
owner=${GHCR_OWNER:-janitochvvll2311}
version=
make_public=0
images=
while [ $# -gt 0 ]; do
    case "$1" in
        --images) images=${2:?`--images` needs a directory}; shift 2 ;;
        --version) version=${2:?`--version` needs a value}; shift 2 ;;
        --owner) owner=${2:?`--owner` needs a value}; shift 2 ;;
        --public) make_public=1; shift ;;
        -h | --help) sed -n '2,12p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
[ -n "$images" ] || { echo "usage: scripts/release-ghcr.sh --images <dir> [--version <version>]" >&2; exit 2; }
[ -d "$images" ] || { echo "no such directory: $images" >&2; exit 2; }
command -v skopeo >/dev/null 2>&1 || { echo "skopeo is not on PATH" >&2; exit 2; }

if [ -z "$version" ]; then
    first=$(find "$images" -maxdepth 1 -name 'anthracite-*-*.tar.gz' | head -1)
    [ -n "$first" ] || { echo "no anthracite-<version>-<variant>.tar.gz in $images" >&2; exit 2; }
    version=$(basename "$first" | sed 's/^anthracite-//; s/-[a-z-]*\.tar\.gz$//')
fi

push() {
    variant=$1
    repository=$2
    moving=$3
    archive=$images/anthracite-$version-$variant.tar.gz
    [ -f "$archive" ] || { echo "missing $archive" >&2; exit 1; }
    echo "ghcr: $variant -> ghcr.io/$owner/$repository:$moving"
    skopeo copy --all "docker-archive:$archive" "docker://ghcr.io/$owner/$repository:$moving"
    skopeo copy --all "docker-archive:$archive" "docker://ghcr.io/$owner/$repository:$version-$variant"
}

push core anthracite-panel core
push base anthracite-panel base
push demo anthracite-panel demo
push demo-sessions anthracite-demo main

if [ "$make_public" = 1 ]; then
    command -v gh >/dev/null 2>&1 || { echo "gh is needed for --public" >&2; exit 2; }
    for package in anthracite-panel anthracite-demo; do
        if gh api -X PATCH "/user/packages/container/$package" \
            -f visibility=public >/dev/null 2>&1; then
            echo "ghcr: $package is public"
        else
            echo "ghcr: could not change $package visibility from the API" >&2
            echo "      set it to Public at https://github.com/users/$owner/packages/container/$package/settings" >&2
        fi
    done
fi

echo "ghcr: pushed $version from $images"
