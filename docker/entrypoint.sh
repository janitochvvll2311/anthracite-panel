#!/bin/sh
set -eu

CONFIG="${ANTH_CONFIG:-/etc/anth/config.toml}"

# The daemon owns the Unix socket and creates the default config file if needed.
anthd --config "$CONFIG" &
ANTHD_PID=$!

stop_daemon() {
    kill "$ANTHD_PID" 2>/dev/null || true
}
trap stop_daemon INT TERM

# Give the daemon a moment to bind its socket before clients connect.
sleep 0.5

if [ "$#" -gt 0 ]; then
    "$@"
    status=$?
else
    anth-panel --config "$CONFIG"
    status=$?
fi

stop_daemon
wait "$ANTHD_PID" 2>/dev/null || true
exit "$status"
