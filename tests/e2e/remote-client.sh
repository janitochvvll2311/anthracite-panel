#!/bin/sh
# Remote-client acid test: the generated Rust and TypeScript clients against a
# running panel. The clients are transport-agnostic; here the transport logs in
# over HTTP and carries the engine session cookie, so the panel is genuinely
# remote from the client process.
#
# ANTH_PANEL_URL=http://127.0.0.1:8090 \
# ANTH_PANEL_OPERATOR=operator \
# ANTH_PANEL_PASSWORD=... \
# tests/e2e/remote-client.sh

set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
url=${ANTH_PANEL_URL:-http://127.0.0.1:8090}
operator=${ANTH_PANEL_OPERATOR:-operator}
: "${ANTH_PANEL_PASSWORD:?ANTH_PANEL_PASSWORD is required}"

echo "remote client: $url"
(cd "$root/sdk/rust" && ANTH_PANEL_URL="$url" ANTH_PANEL_OPERATOR="$operator" \
    ANTH_PANEL_PASSWORD="$ANTH_PANEL_PASSWORD" \
    cargo run --quiet -p anth-client --example remote)
ANTH_PANEL_URL="$url" ANTH_PANEL_OPERATOR="$operator" \
    ANTH_PANEL_PASSWORD="$ANTH_PANEL_PASSWORD" \
    bun "$root/sdk/ts/client/examples/remote.ts"
