#!/bin/sh
# The SDK CLI gate: a project created in /tmp builds, packages, signs and
# verifies without cloning the monorepo.
#
# Usage: scripts/sdk-cli-test.sh
#
# Requires the debug tools: target/debug/{anth,cargo-anth,anth-sign,anth-conformance}.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"
mkdir -p /tmp/anthracite-panel
dir=$(mktemp -d /tmp/anthracite-panel/sdk-cli.XXXXXX)
trap 'rm -rf "$dir"' EXIT

cargo_bin=${CARGO:-cargo}
if ! command -v "$cargo_bin" >/dev/null 2>&1; then
    cargo_bin=/tools/cargo/bin/cargo
fi
bin=$root/target/debug
for tool in anth cargo-anth anth-sign anth-conformance; do
    [ -x "$bin/$tool" ] || {
        echo "missing $bin/$tool; run the debug build first" >&2
        exit 2
    }
done
PATH="$bin:$PATH"
export PATH

"$bin/anth-sign" keygen --out "$dir/dev.key" >/dev/null
public=$("$bin/anth-sign" public-key --key "$dir/dev.key" | awk '/^public_key/ {print $2}')
[ -n "$public" ]

run() {
    lang=$1
    name=$2
    project=$dir/$name
    echo "sdk: $lang project"
    "$bin/anth" app new "org.example.$name" --dir "$project" --lang "$lang"
    (
        cd "$project"
        "$bin/cargo-anth" app build
        "$bin/cargo-anth" app check
        "$bin/cargo-anth" app package --key "$dir/dev.key"
        "$bin/cargo-anth" app verify --public-key "$public"
    )
    [ -f "$project/dist/org.example.$name-0.1.0.anth" ] || {
        echo "no bundle for $name" >&2
        exit 1
    }
}

run rust notes
if command -v bun >/dev/null 2>&1; then
    run bun dashboard
else
    echo "sdk: bun not found; skipped the Bun template" >&2
fi

echo "sdk cli test passed: $dir"
