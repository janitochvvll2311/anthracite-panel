#!/usr/bin/env bash
# Start the demo panel without sudo, run the Playwright suite, then stop it.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export ANTH_BACKEND=demo
export ANTH_ADDR=${ANTH_ADDR:-127.0.0.1:8080}
export E2E_BASE_URL=${E2E_BASE_URL:-http://127.0.0.1:5173}
run=/tmp/anthracite-panel/demo
log=/tmp/anthracite-panel/e2e

if [ -r /tools/env.sh ]; then
  # shellcheck disable=SC1091
  . /tools/env.sh
fi

mkdir -p "$log"
cleanup() {
  ANTH_BACKEND=demo "$root/scripts/dev.sh" stop >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "e2e: starting the demo panel"
"$root/scripts/dev.sh" start >"$log/start.log" 2>&1
binary=$root/backend/target/x86_64-unknown-linux-musl/debug/anth-panel
if [ ! -x "$binary" ]; then
  echo "e2e: missing $binary" >&2
  exit 1
fi
"$binary" --config "$run/config.toml" config set auth.enabled false >/dev/null
"$root/scripts/dev.sh" restart >"$log/restart.log" 2>&1

cd "$root/tests"
if ! [ -d node_modules ]; then
  bun install --frozen-lockfile
fi
bun run test "$@"
