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
export ANTHRACITE_RELEASE=1
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
invoked=$(pwd)
cd "$root"

apps_all="hello accounts firewall fs home net proc storage terminal certs acme proxy containers store"
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
    cargo build --manifest-path sdk/rust/Cargo.toml --release -p anth-cli -p cargo-anth -p anth-conformance
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
for binary in anth anth-engine anth-panel anth-sign anth-conformance cargo-anth; do
    cp "target/release/$binary" "$out/bin/$binary"
    sha=$(sha256sum "$out/bin/$binary" | cut -d' ' -f1)
    binary_rows="$binary_rows$binary	$sha
"
done

mkdir -p "$out/bundles"
# Signs one staged directory, verifies the tier and leaves the id, version and
# digest in `bundle_*` for the loop that calls it.
sign_one() {
    unsigned=$1
    target=$2
    label=$3
    stage=$tmp/stage-$label
    mkdir -p "$stage"
    unzip -q "$unsigned" -d "$stage"
    ./target/release/anth-sign "$stage" --key "$key" >/dev/null
    rm -f "$target"
    (cd "$stage" && zip -qr "$target" .)

    report=$(./target/release/anth-conformance --require-signature --key "$public" "$target")
    if ! printf '%s\n' "$report" | grep -qx 'tier system'; then
        echo "the signed $label bundle did not earn the system tier:" >&2
        printf '%s\n' "$report" >&2
        exit 1
    fi
    bundle_id=$(printf '%s\n' "$report" | head -1 | cut -d' ' -f1)
    bundle_version=$(printf '%s\n' "$report" | head -1 | cut -d' ' -f2)
    bundle_sha=$(sha256sum "$target" | cut -d' ' -f1)
    echo "release: $bundle_id $bundle_version -> $target"
}

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
    sign_one "$unsigned" "$out/bundles/$name.anth" "$name"
    rows="$rows$bundle_id	$bundle_version	$name.anth	$bundle_sha
"
done

echo "release: building and signing the demo emulator bundles"
cargo build --manifest-path demo/emulator/Cargo.toml --release \
    --bin host-emulator --bin bundle-emulator
./target/release/bundle-emulator --binary ./target/release/host-emulator \
    --out "$tmp/demo" >/dev/null
mkdir -p "$out/demo"
demo_rows=
for name in accounts firewall fs home net proc storage terminal; do
    [ -f "$tmp/demo/$name.anth" ] || { echo "the emulator did not build $name" >&2; exit 1; }
    sign_one "$tmp/demo/$name.anth" "$out/demo/$name.anth" "demo-$name"
    demo_rows="$demo_rows$bundle_id	$bundle_version	$name.anth	$bundle_sha
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
    printf '\n  ],\n'
    printf '  "demo": [\n'
    first=1
    printf '%s' "$demo_rows" | while IFS='	' read -r id version file sha; do
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

(cd "$out" && sha256sum bin/* bundles/* demo/* > SHA256SUMS)

echo "release: wrote $out/manifest.json and $out/SHA256SUMS ($release_version, key $fingerprint)"
