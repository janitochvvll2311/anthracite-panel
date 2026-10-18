#!/bin/sh
set -eu

CONFIG="${ANTH_CONFIG:-/etc/anth/config.toml}"

# `run` starts the daemon and the HTTP API together; exec keeps signal
# handling with the panel so the container stops cleanly.
if [ "$#" -gt 0 ]; then
    exec anth-panel --config "$CONFIG" "$@"
fi

exec anth-panel --config "$CONFIG" run
