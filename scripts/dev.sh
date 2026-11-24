#!/bin/sh
# The panel dev server: the official shell against the developer demo.
#
#   scripts/dev.sh                                # 127.0.0.1:5173, loopback only
#   ANTH_DEV_LISTEN=0.0.0.0:5173 scripts/dev.sh   # behind a preview proxy
#   ANTH_DEV_CONFIG=demo/config.toml scripts/dev.sh
#
# The demo config disables signature checks, so the default is loopback;
# a non-loopback address is an explicit choice and turns on --allow-remote.
# The shell is rebuilt when any source is newer than the served module, because
# the debug backend reads panel/frontend/dist from disk.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"

listen=${ANTH_DEV_LISTEN:-127.0.0.1:5173}
config=${ANTH_DEV_CONFIG:-demo/config.toml}
remote=
case "$listen" in
    127.0.0.1:*|localhost:*) ;;
    *) remote=--allow-remote ;;
esac

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

if [ ! -f panel/frontend/dist/main.js ] \
    || [ -n "$(find panel/frontend/src -newer panel/frontend/dist/main.js 2>/dev/null | head -1)" ]; then
    echo "dev: building the panel shell"
    (
        cd panel/frontend
        "$bun_bin" install
        "$bun_bin" run build
    )
fi

echo "dev: http://$listen (config $config)"
set -- --config "$config" --listen "$listen"
if [ -n "$remote" ]; then
    set -- "$@" --allow-remote
fi
exec "$cargo_bin" run --manifest-path panel/backend/Cargo.toml -- "$@"
