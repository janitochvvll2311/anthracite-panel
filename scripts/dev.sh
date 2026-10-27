#!/usr/bin/env bash
#
# Runs the daemon and the Vite dev server together.
#
#   ./scripts/dev.sh start     # cargo run -p anth-panel -- run + bun run dev
#   ./scripts/dev.sh stop
#   ./scripts/dev.sh status
#   ./scripts/dev.sh restart
#
# Set ANTH_HMR=on for Vite fast refresh on desktop. HMR is off by default
# because Android drops the HMR socket while the native file picker is open and
# Vite reloads the page when it reconnects, losing the selected file.
export ANTH_HMR=${ANTH_HMR:-off}

# PIDs, logs and the debug config live under /tmp/anthracite-panel/dev, so the
# tree stays clean and a run never inherits a stale socket from another one.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
run=/tmp/anthracite-panel/dev
config=$run/config.toml
backend_log=$run/backend.log
frontend_log=$run/frontend.log
backend_pid=$run/backend.pid
frontend_pid=$run/frontend.pid

backend_addr=${ANTH_ADDR:-0.0.0.0:8080}
vite_host=${ANTH_VITE_HOST:-0.0.0.0}
vite_port=${ANTH_VITE_PORT:-5173}
health_host=${backend_addr/0.0.0.0/127.0.0.1}
health_url="http://$health_host/healthz"

mkdir -p "$run"

read_pid() {
  if [ -f "$1" ]; then
    cat "$1"
  else
    printf ''
  fi
}

alive() {
  local pid=$1
  [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null
}

# Lists the direct children of a pid from /proc, so a killed `cargo run` does
# not leave the compiled daemon behind (and vice versa for `bun` and `vite`).
children_of() {
  local pid=$1
  if [ -r "/proc/$pid/task/$pid/children" ]; then
    cat "/proc/$pid/task/$pid/children"
  fi
}

kill_tree() {
  local signal=$1
  local pid=$2
  local child
  for child in $(children_of "$pid"); do
    kill_tree "$signal" "$child"
  done
  kill "-$signal" "$pid" 2>/dev/null || true
  return 0
}

wait_for() {
  local url=$1
  local attempts=${2:-120}
  local i
  for i in $(seq 1 "$attempts"); do
    if curl -sf -o /dev/null "$url" 2>/dev/null; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}

# Warn once per start when the host cannot exercise the network mutations. The
# panel still serves the inventory; only ip/nft changes need these.
check_network_tools() {
  local warnings=()
  if ! ip -V 2>&1 | grep -qi iproute2; then
    warnings+=("ip is not iproute2; network mutations need iproute2 (BusyBox ip cannot add or delete links)")
  fi
  if ! command -v nft >/dev/null 2>&1; then
    warnings+=("nft is not installed; the firewall slice will need nftables")
  fi
  local effective
  effective=$(sed -n 's/^CapEff:[[:space:]]*//p' /proc/self/status 2>/dev/null)
  if [ -n "$effective" ] && [ $((0x$effective & 0x1000)) -eq 0 ]; then
    warnings+=("CAP_NET_ADMIN is missing; ip and nft changes will be refused (start the container with --cap-add=NET_ADMIN)")
  fi
  local warning
  for warning in "${warnings[@]}"; do
    echo "warning: $warning" >&2
  done
}

launch() {
  local name=$1
  local dir=$2
  local log=$3
  shift 3
  # `exec` makes the background subshell the command itself, so `$!` is the
  # pid that `kill_tree` starts from.
  ( cd "$dir" && exec "$@" ) >"$log" 2>&1 &
  echo $! > "$run/$name.pid"
}

stop_one() {
  local name=$1
  local file=$run/$1.pid
  local pid
  pid=$(read_pid "$file")
  if ! alive "$pid"; then
    echo "$name: not running"
    rm -f "$file"
    return 0
  fi

  kill_tree TERM "$pid"
  local i
  for i in $(seq 1 50); do
    if ! alive "$pid"; then
      break
    fi
    sleep 0.1
  done
  if alive "$pid"; then
    kill_tree KILL "$pid"
  fi
  rm -f "$file"
  echo "$name: stopped"
}

start_one() {
  local name=$1
  local file=$run/$1.pid
  local pid
  pid=$(read_pid "$file")
  if alive "$pid"; then
    echo "$name: already running (pid $pid)"
    return 0
  fi
  rm -f "$file"
}

start() {
  check_network_tools
  start_one backend
  if ! alive "$(read_pid "$backend_pid")"; then
    echo "backend: starting on $backend_addr"
    launch backend "$root/backend" "$backend_log" \
      cargo run -q -p anth-panel -- run --addr "$backend_addr" --config "$config"
  fi

  start_one frontend
  if ! alive "$(read_pid "$frontend_pid")"; then
    echo "frontend: starting Vite on $vite_host:$vite_port"
    launch frontend "$root/frontend" "$frontend_log" \
      bun run dev --host "$vite_host" --port "$vite_port"
  fi

  if wait_for "$health_url"; then
    echo "backend:  $health_url  (log: $backend_log)"
  else
    echo "backend:  not answering yet; see $backend_log" >&2
    tail -n 10 "$backend_log" >&2 || true
  fi

  if wait_for "http://127.0.0.1:$vite_port/"; then
    echo "frontend: http://localhost:$vite_port/  (log: $frontend_log)"
  else
    echo "frontend: not answering yet; see $frontend_log" >&2
    tail -n 10 "$frontend_log" >&2 || true
  fi

  echo "stop both with: $0 stop"
}

stop() {
  stop_one frontend
  stop_one backend
}

status() {
  local backend
  local frontend
  backend=$(read_pid "$backend_pid")
  frontend=$(read_pid "$frontend_pid")
  if alive "$backend"; then
    echo "backend:  running (pid $backend) $health_url"
  else
    echo "backend:  stopped"
  fi
  if alive "$frontend"; then
    echo "frontend: running (pid $frontend) http://localhost:$vite_port/"
  else
    echo "frontend: stopped"
  fi
  exit 0
}

case "${1:-start}" in
  start | up)
    start
    ;;
  stop | down)
    stop
    ;;
  restart)
    stop
    start
    ;;
  status)
    status
    ;;
  *)
    echo "usage: $0 {start|stop|restart|status}" >&2
    exit 2
    ;;
esac
