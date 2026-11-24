#!/bin/sh
# The panel dev server: the official shell against the developer demo.
#
#   scripts/dev.sh                          # 0.0.0.0:5173, demo/config.toml
#   ANTH_DEV_LISTEN=127.0.0.1:5173 scripts/dev.sh
#   ANTH_DEV_CONFIG=demo/config.toml scripts/dev.sh
#
# The debug backend reads panel/frontend/dist from disk, so the shell is built
# once when it is missing. `--allow-remote` is on because a dev server behind a
# preview proxy is reached from another host; sessions still protect it.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"

listen=${ANTH_DEV_LISTEN:-0.0.0.0:5173}
config=${ANTH_DEV_CONFIG:-demo/config.toml}

bun_bin=${BUN:-bun}
if ! command -v "$bun_bin" >/dev/null 2>&1; then
    if [ -x /tools/bun/bin/bun ]; then
        bun_bin=/tools/bun/bin/bun
    fi
fi
cargo_bin=${CARGO:-cargo}
if ! command -v "$cargo_bin" >/dev/null 2>&1; then
    if [ -x /tools/cargo/bin/cargo ]; then
        cargo_bin=/tools/cargo/bin/cargo
    fi
fi

if [ ! -f panel/frontend/dist/main.js ]; then
    echo "dev: building the panel shell"
    (
        cd panel/frontend
        "$bun_bin" install
        "$bun_bin" run build
    )
fi

echo "dev: http://$listen (config $config)"
exec "$cargo_bin" run --manifest-path panel/backend/Cargo.toml -- \
    --config "$config" --listen "$listen" --allow-remote
