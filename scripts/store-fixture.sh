#!/bin/sh
# Build a signed store fixture: a key, a signed hello bundle and a signed index
# under <shared>/store/airgap. With <store-data> it also writes the store's
# sources.json (the app must be installed and stopped first).
#
#   scripts/store-fixture.sh <shared-dir> [<store-data-dir>]
#
# The key lives in /tmp/anthracite-panel/store-fixture/ (0600). The script prints
# STORE_PUBLIC_KEY for the manager signer handoff.
set -eu

shared=${1:?usage: store-fixture.sh <shared-dir> [<store-data-dir>]}
store_data=${2:-}
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
fixture=/tmp/anthracite-panel/store-fixture
mkdir -p "$fixture" "$shared/store/airgap"

anth_sign=${ANTH_SIGN:-$root/target/debug/anth-sign}
if [ ! -x "$anth_sign" ]; then
    (cd "$root/sdk/rust" && cargo build -p anth-signer --bin anth-sign)
    anth_sign=$root/target/debug/anth-sign
fi

if [ ! -f "$fixture/store.key" ]; then
    "$anth_sign" keygen --out "$fixture/store.key" >/dev/null
    chmod 600 "$fixture/store.key"
fi

(cd "$root" && scripts/build-app.sh apps/hello >/dev/null)
(cd "$root" && ANTHRACITE_SIGNING_KEY="$fixture/store.key" ANTH_SIGN="$anth_sign" \
    scripts/package-app.sh apps/hello "$fixture/hello.anth" >/dev/null)

public=$("$anth_sign" public-key --key "$fixture/store.key" | awk '$1 == "public_key" {print $2}')
fingerprint=$("$anth_sign" public-key --key "$fixture/store.key" | awk '$1 == "fingerprint" {print $2}')
size=$(stat -c %s "$fixture/hello.anth")
digest=$(sha256sum "$fixture/hello.anth" | cut -d' ' -f1)

cat > "$fixture/index.json" <<EOF
{
  "schema": 1,
  "store": { "id": "org.example.store", "name": "Example Store" },
  "apps": [
    {
      "id": "org.anthracite.hello",
      "name": "Hello",
      "summary": "A tiny greeting app.",
      "description": "The vertical-slice app: one interface and one view.",
      "publisher": "Example",
      "license": "Apache-2.0",
      "homepage": "https://example.test/hello",
      "categories": ["examples"],
      "versions": [
        {
          "version": "0.1.0",
          "channel": "stable",
          "published_at": 1795000000,
          "sha256": "$digest",
          "size": $size,
          "bundle_url": "hello.anth",
          "signer": "$fingerprint",
          "yanked": false
        }
      ]
    }
  ]
}
EOF
"$anth_sign" file "$fixture/index.json" --key "$fixture/store.key" >/dev/null

cp "$fixture/index.json" "$fixture/index.json.sig" "$fixture/hello.anth" \
    "$shared/store/airgap/"
chmod -R a+rX "$shared/store"

if [ -n "$store_data" ]; then
    cat > "$store_data/sources.json" <<EOF
{
  "sources": [
    {
      "id": "airgap",
      "name": "Air-gapped",
      "kind": "local",
      "url": "airgap",
      "public-key": "$public"
    }
  ]
}
EOF
    chmod 600 "$store_data/sources.json"
    chown "$(stat -c %u "$store_data"):$(stat -c %g "$store_data")" "$store_data/sources.json"
fi

echo "STORE_PUBLIC_KEY=$public"
echo "STORE_SIGNER=$fingerprint"
echo "STORE_INDEX=$shared/store/airgap/index.json"
