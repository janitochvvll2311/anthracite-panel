#!/bin/sh
# Remote-client acid test: the generated Rust and TypeScript clients against a
# running panel. The clients are transport-agnostic; here the transport is HTTP
# with Basic auth, so the panel is genuinely remote from the client process.
#
# ANTH_PANEL_URL=http://127.0.0.1:8090 \
# ANTH_PANEL_AUTH=operator:anthracite-dev \
# tests/e2e/remote-client.sh

set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
url=${ANTH_PANEL_URL:-http://127.0.0.1:8090}
auth=${ANTH_PANEL_AUTH:-operator:anthracite-dev}

echo "remote client: $url"
(cd "$root/sdk/rust" && ANTH_PANEL_URL="$url" ANTH_PANEL_AUTH="$auth" \
    cargo run --quiet -p anth-client --example remote)
ANTH_PANEL_URL="$url" ANTH_PANEL_AUTH="$auth" \
    bun "$root/sdk/ts/client/examples/remote.ts"
