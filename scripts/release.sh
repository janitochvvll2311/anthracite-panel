#!/bin/sh
# Build a release: the binaries that trust a key and the bundles that key signs,
# checked against each other (§9.1).
#
# Usage: scripts/release.sh --key <private-key> [--out <dir>] [--apps <a,b,...>]
#
# The private key stays on this machine: the script reads it to derive the public
# half, compiles that half into the binaries and signs every bundle with the
# private one. It refuses to finish if any bundle is not `system` or if the built
# engine reports another fingerprint. The key is never printed.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
invoked=$(pwd)
cd "$root"

apps_all="hello accounts firewall fs home net proc storage terminal"
key=
out=
apps=$apps_all
while [ $# -gt 0 ]; do
    case "$1" in
        --key)
            value=${2:?`--key` needs a path}
            case "$value" in
                /*) key=$value ;;
                *) key=$invoked/$value ;;
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
        --apps)
            apps=${2:?`--apps` needs a comma-separated list}
            shift 2
            ;;
        -h | --help)
            sed -n '2,10p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done
[ -n "$key" ] || {
    echo "usage: scripts/release.sh --key <private-key> [--out <dir>] [--apps <a,b,...>]" >&2
    exit 2
}
[ -n "$out" ] || out=$root/release
[ -f "$key" ] || { echo "no such key: $key" >&2; exit 2; }
[ -f panel/frontend/dist/index.html ] || {
    echo "the panel frontend is not built: run 'bun run build' in panel/frontend" >&2
    exit 2
}

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/release.XXXXXX)
engine=
trap 'if [ -n "$engine" ]; then kill "$engine" 2>/dev/null || true; fi; rm -rf "$tmp"' EXIT

echo "release: deriving the public key"
cargo build --manifest-path sdk/rust/Cargo.toml --release -p anth-signer
key_info=$(./target/release/anth-sign public-key --key "$key")
public=$(printf '%s\n' "$key_info" | awk '/^public_key/ {print $2}')
fingerprint=$(printf '%s\n' "$key_info" | awk '/^fingerprint/ {print $2}')
[ -n "$public" ] || { echo "the key has no public half" >&2; exit 1; }
echo "release: key $fingerprint"

echo "release: building the binaries with the key compiled in"
ANTHRACITE_DISTRIBUTION_KEY=$public \
    cargo build --manifest-path sdk/rust/Cargo.toml --release -p anth-cli -p anth-conformance
ANTHRACITE_DISTRIBUTION_KEY=$public \
    cargo build --manifest-path engine/Cargo.toml --release -p anth-engine
ANTHRACITE_DISTRIBUTION_KEY=$public \
    cargo build --manifest-path panel/backend/Cargo.toml --release

echo "release: checking that the engine trusts the signing key"
mkdir -p "$tmp/engine"
cat > "$tmp/engine/config.toml" <<EOF
version = 1
socket = "$tmp/engine/engine.sock"
state = "$tmp/engine/state"
EOF
./target/release/anth-engine --config "$tmp/engine/config.toml" > "$tmp/engine/engine.log" 2>&1 &
engine=$!
reported=
for _ in $(seq 1 100); do
    if ./target/release/anth --socket "$tmp/engine/engine.sock" key distribution \
        > "$tmp/engine/distribution" 2>/dev/null; then
        reported=$(awk '/^fingerprint/ {print $2}' "$tmp/engine/distribution")
        break
    fi
    sleep 0.1
done
if [ "$reported" != "$fingerprint" ]; then
    echo "the built engine trusts '${reported:-nothing}', not $fingerprint" >&2
    cat "$tmp/engine/engine.log" >&2
    exit 1
fi
kill "$engine"
wait "$engine" 2>/dev/null || true
engine=

release_version=$(./target/release/anth --version | awk '{print $2}')
echo "release: staging the binaries ($release_version)"
mkdir -p "$out/bin"
binary_rows=
for binary in anth anth-engine anth-panel; do
    cp "target/release/$binary" "$out/bin/$binary"
    sha=$(sha256sum "$out/bin/$binary" | cut -d' ' -f1)
    binary_rows="$binary_rows$binary	$sha
"
done

mkdir -p "$out/bundles"
rows=
for name in $(printf '%s' "$apps" | tr ',' ' '); do
    [ -d "apps/$name" ] || { echo "no such app: $name" >&2; exit 2; }
    unsigned=$tmp/$name.anth
    if [ "$name" = hello ]; then
        scripts/build-app.sh apps/hello
        scripts/package-app.sh apps/hello "$unsigned"
    else
        "apps/$name/build.sh" "$unsigned"
    fi

    stage=$tmp/stage-$name
    mkdir -p "$stage"
    unzip -q "$unsigned" -d "$stage"
    ./target/release/anth-sign "$stage" --key "$key" >/dev/null
    rm -f "$out/bundles/$name.anth"
    (cd "$stage" && zip -qr "$out/bundles/$name.anth" .)

    report=$(./target/release/anth-conformance --require-signature --key "$public" \
        "$out/bundles/$name.anth")
    if ! printf '%s\n' "$report" | grep -qx 'tier system'; then
        echo "the signed $name bundle did not earn the system tier:" >&2
        printf '%s\n' "$report" >&2
        exit 1
    fi
    id=$(printf '%s\n' "$report" | head -1 | cut -d' ' -f1)
    version=$(printf '%s\n' "$report" | head -1 | cut -d' ' -f2)
    sha=$(sha256sum "$out/bundles/$name.anth" | cut -d' ' -f1)
    echo "release: $id $version -> $out/bundles/$name.anth"
    rows="$rows$id	$version	$name.anth	$sha
"
done

{
    printf '{\n'
    printf '  "version": "%s",\n' "$release_version"
    printf '  "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '  "binaries": [\n'
    first=1
    printf '%s' "$binary_rows" | while IFS='	' read -r name sha; do
        [ -n "$name" ] || continue
        if [ "$first" = 1 ]; then
            first=0
        else
            printf ',\n'
        fi
        printf '    {"name": "%s", "sha256": "%s"}' "$name" "$sha"
    done
    printf '\n  ],\n'
    printf '  "distribution_key": {"public_key": "%s", "fingerprint": "%s"},\n' \
        "$public" "$fingerprint"
    printf '  "apps": [\n'
    first=1
    printf '%s' "$rows" | while IFS='	' read -r id version file sha; do
        [ -n "$id" ] || continue
        if [ "$first" = 1 ]; then
            first=0
        else
            printf ',\n'
        fi
        printf '    {"id": "%s", "version": "%s", "file": "%s", "sha256": "%s", "tier": "system"}' \
            "$id" "$version" "$file" "$sha"
    done
    printf '\n  ]\n}\n'
} > "$out/manifest.json"

(cd "$out" && sha256sum bin/* bundles/* > SHA256SUMS)

echo "release: wrote $out/manifest.json and $out/SHA256SUMS ($release_version, key $fingerprint)"
