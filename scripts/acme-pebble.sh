#!/bin/sh
# Prepare (and optionally run) Pebble, the real ACME QA gate.
#
#   scripts/acme-pebble.sh          # download and print the next commands
#   scripts/acme-pebble.sh --run    # start Pebble in the foreground
#
# The automated gate is the loopback fixture in apps/acme/backend/tests; this
# script is what a release QA row runs against a real RFC 8555 server.
set -eu

version=v2.10.3
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
dest=/tmp/anthracite-panel/pebble
mkdir -p "$dest"

case "$(uname -m)" in
    x86_64 | amd64) arch=amd64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *) echo "unsupported architecture: $(uname -m)" >&2; exit 2 ;;
esac

if [ ! -x "$dest/pebble" ]; then
    echo "downloading pebble $version ($arch)…"
    curl -fsSL -o "$dest/pebble.tar.gz" \
        "https://github.com/letsencrypt/pebble/releases/download/$version/pebble-linux-$arch.tar.gz"
    tar -xzf "$dest/pebble.tar.gz" -C "$dest" --strip-components=3
    rm -f "$dest/pebble.tar.gz"
    chmod +x "$dest/pebble"
fi

if [ ! -f "$dest/test/certs/pebble.minica.pem" ]; then
    echo "downloading the Pebble test certificates…"
    curl -fsSL -o "$dest/source.tar.gz" \
        "https://github.com/letsencrypt/pebble/archive/refs/tags/$version.tar.gz"
    tmp=$(mktemp -d /tmp/anthracite-panel/pebble-src.XXXXXX)
    trap 'rm -rf "$tmp"' EXIT
    tar -xzf "$dest/source.tar.gz" -C "$tmp" --strip-components=1
    cp -R "$tmp/test" "$dest/test"
    rm -rf "$tmp" "$dest/source.tar.gz"
    trap - EXIT
fi

cat > "$dest/pebble-config.json" <<EOF
{
  "pebble": {
    "listenAddress": "localhost:14000",
    "managementListenAddress": "localhost:15000",
    "certificate": "$dest/test/certs/localhost/cert.pem",
    "privateKey": "$dest/test/certs/localhost/key.pem",
    "httpPort": 5002,
    "tlsPort": 5001,
    "ocspResponderURL": "",
    "externalAccountBindingRequired": false,
    "keyAlgorithm": "ecdsa"
  }
}
EOF

if [ "${1:-}" = "--run" ]; then
    cd "$dest"
    PEBBLE_VA_NOSLEEP=1 exec ./pebble -config pebble-config.json
fi

cat <<EOF
Pebble is ready.

1. Start it (it needs httpPort 5002 and https 14000 free):
   scripts/acme-pebble.sh --run

2. In another shell, run the real gate:
   ANTH_PEBBLE_DIRECTORY=https://localhost:14000/dir \\
   ANTH_PEBBLE_CA=$dest/test/certs/pebble.minica.pem \\
   (cd $root/apps/acme/backend && cargo test --test pebble -- --ignored --nocapture)

Let's Encrypt staging remains a manual QA row; it is not part of CI.
EOF
