#!/bin/sh
# Publish the signed bundles of a release on their own: one `.anth` per app for
# the system set and for the emulator set, plus a tarball with all of them and
# their checksums. The variants already carry the same bundles under `bundles/`;
# this is the per-app path for an operator who wants one app instead of a whole
# variant.
#
# Usage: scripts/release-bundles.sh --release <signed-release-dir> [--out <dir>]
#
# Every bundle is checked with `anth-conformance --require-signature` against the
# distribution key recorded in the release manifest, so a bundle that is not
# `system` stops the run. The default `--out` is `<release>/separate`.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
invoked=$(pwd)
cd "$root"

release=
out=
while [ $# -gt 0 ]; do
    case "$1" in
        --release)
            value=${2:?`--release` needs a path}
            case "$value" in
                /*) release=$value ;;
                *) release=$invoked/$value ;;
            esac
            shift 2
            ;;
        --out)
            value=${2:?`--out` needs a directory}
            case "$value" in
                /*) out=$value ;;
                *) out=$invoked/$value ;;
            esac
            shift 2
            ;;
        -h | --help)
            sed -n '2,12p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done
[ -n "$release" ] || {
    echo "usage: scripts/release-bundles.sh --release <signed-release-dir> [--out <dir>]" >&2
    exit 2
}
[ -d "$release/bundles" ] || {
    echo "no system bundles in $release; run scripts/release.sh first" >&2
    exit 2
}
[ -d "$release/demo" ] || {
    echo "no demo bundles in $release; run scripts/release.sh first" >&2
    exit 2
}
[ -f "$release/manifest.json" ] || {
    echo "no manifest.json in $release" >&2
    exit 2
}
[ -x target/release/anth-conformance ] || {
    echo "no target/release/anth-conformance; run scripts/release.sh first" >&2
    exit 2
}

version=$(sed -n 's/^  "version": "\([^"]*\)",$/\1/p' "$release/manifest.json" | head -1)
public=$(sed -n 's/.*"public_key": "\([0-9a-f]*\)".*/\1/p' "$release/manifest.json" | head -1)
[ -n "$version" ] || { echo "the release manifest has no version" >&2; exit 1; }
[ -n "$public" ] || { echo "the release manifest has no public key" >&2; exit 1; }

out=${out:-$release/separate}
rm -rf "$out"
mkdir -p "$out"

apps=""
for bundle in "$release/bundles"/*.anth; do
    app=$(basename "$bundle" .anth)
    # Hello is the developer example, not part of the system set.
    [ "$app" = hello ] && continue
    apps="$apps $app"
done
[ -n "$apps" ] || { echo "no bundles in $release/bundles" >&2; exit 1; }
list=$out/.bundles
: > "$list"
for app in $apps; do
    source=$release/bundles/$app.anth
    [ -f "$source" ] || { echo "missing $source" >&2; exit 1; }
    cp "$source" "$out/system-org.anthracite.$app.anth"
    echo "./system-org.anthracite.$app.anth" >> "$list"
    if [ -f "$release/demo/$app.anth" ]; then
        cp "$release/demo/$app.anth" "$out/demo-org.anthracite.$app.anth"
        echo "./demo-org.anthracite.$app.anth" >> "$list"
    else
        case " accounts firewall fs home net proc storage terminal " in
            *" $app "*)
                echo "missing demo bundle $release/demo/$app.anth" >&2
                exit 1
                ;;
        esac
    fi
done

for bundle in "$out"/*.anth; do
    report=$(./target/release/anth-conformance \
        --require-signature --key "$public" "$bundle")
    if ! printf '%s\n' "$report" | grep -qx 'tier system'; then
        echo "the bundle $(basename "$bundle") did not earn the system tier:" >&2
        printf '%s\n' "$report" >&2
        exit 1
    fi
done

count=$(wc -l < "$list" | tr -d ' ')
(
    cd "$out"
    xargs sha256sum < .bundles > SHA256SUMS
    tar -czf "anthracite-$version-bundles.tar.gz" -T .bundles SHA256SUMS
    rm -f .bundles
)
echo "bundles: wrote $out ($version, $count bundles, key $(printf '%s' "$public" | cut -c1-12)…)"
echo "bundles: $out/anthracite-$version-bundles.tar.gz"
