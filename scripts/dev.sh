#!/usr/bin/env bash
#
# Runs the daemon and the Vite dev server together.
#
#   ./scripts/dev.sh start     # cargo build + daemon + bun run dev
#   ./scripts/dev.sh stop
#   ./scripts/dev.sh status
#   ./scripts/dev.sh restart
#
# Network mutations and the firewall need a daemon with CAP_NET_ADMIN, so the
# daemon always runs through sudo: the helper builds the binary as your own
# user, then launches it from the directory you invoked the script in. `stop`
# and `restart` signal the root daemon through sudo too. Sudo does not add a
# capability the container's bounding set excludes.
#
# Set ANTH_HMR=on for Vite fast refresh on desktop. HMR is off by default
# because Android drops the HMR socket while the native file picker is open and
# Vite reloads the page when it reconnects, losing the selected file.
export ANTH_HMR=${ANTH_HMR:-off}

# PIDs, logs and the debug config live under /tmp/anthracite-panel/dev, so the
# tree stays clean and a run never inherits a stale socket from another one.
set -euo pipefail

# `sudo` resets PATH on many hosts. Source the toolchain paths here so the
# cargo and bun wrappers are found, including after the SUDO_USER hand-back.
if [ -r /tools/env.sh ]; then
  # shellcheck disable=SC1091
  . /tools/env.sh
fi

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
script_path=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/$(basename -- "${BASH_SOURCE[0]}")
# The daemon runs from the directory the helper was called in, not from the
# repo, so relative config or plugin paths the caller expects still resolve.
invoke_dir=$PWD
backend=${ANTH_BACKEND:-real}
case "$backend" in
  demo) run=/tmp/anthracite-panel/demo ;;
  real) run=/tmp/anthracite-panel/dev ;;
  *) echo "error: ANTH_BACKEND must be real or demo" >&2; exit 2 ;;
esac
export ANTH_BACKEND=$backend
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

# Elevation model: only the daemon runs as root. `sudo ./scripts/dev.sh ...`
# arrives as root with SUDO_USER set, so hand the helper back to that account
# and let the daemon launch elevate itself. Running the whole helper as root
# would leave root-owned build output, node_modules and run files behind and
# break the next plain run.
if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
  exec sudo -u "$SUDO_USER" -H env \
    ANTH_HMR="$ANTH_HMR" \
    ANTH_ADDR="$backend_addr" \
    ANTH_VITE_HOST="$vite_host" \
    ANTH_VITE_PORT="$vite_port" \
    "$script_path" "$@"
fi

# Prefix for the daemon launch. A root shell needs no wrapper; `-n` keeps a
# missing password from turning into an invisible prompt (see sudo_ready).
daemon_prefix=()
if [ "$backend" = real ] && [ "$(id -u)" -ne 0 ]; then
  daemon_prefix=(sudo -n)
fi

if [ "$backend" = demo ]; then
  echo "backend: demo (all host features simulated; no sudo)" >&2
  echo "warning: this slice simulates every host feature; no privileged command runs" >&2
fi

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
  local state
  [ -n "$pid" ] || return 1
  # `/proc` is readable across uids, so status sees a daemon started under sudo.
  # A zombie still has a /proc entry but cannot be stopped; treat it as gone so
  # a stale pid file does not block the next start.
  [ -r "/proc/$pid/stat" ] || return 1
  state=$(cut -d' ' -f3 "/proc/$pid/stat" 2>/dev/null || true)
  [ "$state" != "Z" ] && [ "$state" != "X" ]
}

proc_uid() {
  sed -n 's/^Uid:[[:space:]]*\([0-9][0-9]*\).*/\1/p' "/proc/$1/status" 2>/dev/null || true
}

# A pid owned by another account (the sudo daemon) needs sudo to be signalled.
is_foreign() {
  local pid=$1
  local uid
  [ "$(id -u)" -eq 0 ] && return 1
  uid=$(proc_uid "$pid")
  [ -n "$uid" ] && [ "$uid" != "$(id -u)" ]
}

# Makes sure a later `sudo -n` succeeds. A terminal gets one explicit password
# prompt; a non-interactive caller fails with an actionable message instead of
# looking hung. The cached timestamp covers the daemon launch and the stop.
sudo_ready() {
  [ "$(id -u)" -eq 0 ] && return 0
  if ! command -v sudo >/dev/null 2>&1; then
    echo "error: elevating the daemon needs sudo on PATH" >&2
    return 1
  fi
  if sudo -n true 2>/dev/null; then
    return 0
  fi
  if [ -t 0 ] && [ -t 2 ]; then
    echo "dev: sudo needs a password; asking once..." >&2
    sudo -v
    return
  fi
  echo "error: sudo needs a password but stdin is not a terminal" >&2
  echo "       run this from a terminal or configure NOPASSWD for the dev helper" >&2
  return 1
}

# Lists the direct children of a pid from /proc, so a killed `cargo run` does
# not leave the compiled daemon behind (and vice versa for `bun` and `vite`).
children_of() {
  local pid=$1
  if [ -r "/proc/$pid/task/$pid/children" ]; then
    cat "/proc/$pid/task/$pid/children"
  fi
}

signal_pid() {
  local signal=$1
  local pid=$2
  if is_foreign "$pid"; then
    sudo -n kill "-$signal" "$pid" 2>/dev/null || true
  else
    kill "-$signal" "$pid" 2>/dev/null || true
  fi
}

kill_tree() {
  local signal=$1
  local pid=$2
  local child
  for child in $(children_of "$pid"); do
    kill_tree "$signal" "$child"
  done
  signal_pid "$signal" "$pid"
  return 0
}

wait_for() {
  local url=$1
  local attempts=${2:-120}
  local started=$SECONDS
  local i
  for i in $(seq 1 "$attempts"); do
    # A server that accepts the socket but never answers must not hang the
    # helper; each probe is bounded and the whole wait has a deadline.
    if curl -sf --connect-timeout 1 --max-time 2 -o /dev/null "$url" 2>/dev/null; then
      return 0
    fi
    if [ $((SECONDS - started)) -ge 60 ]; then
      return 1
    fi
    sleep 0.5
  done
  return 1
}

# Effective capabilities of the root process the daemon will run as.
effective_caps() {
  if [ "$(id -u)" -eq 0 ]; then
    sed -n 's/^CapEff:[[:space:]]*//p' /proc/self/status 2>/dev/null || true
  else
    sudo -n sed -n 's/^CapEff:[[:space:]]*//p' /proc/self/status 2>/dev/null || true
  fi
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
  effective=$(effective_caps)
  if [ -n "$effective" ] && [ $((0x$effective & 0x1000)) -eq 0 ]; then
    warnings+=("CAP_NET_ADMIN is missing even for root; ip and nft changes will be refused (start the container with --cap-add=NET_ADMIN)")
  fi
  local warning
  for warning in "${warnings[@]}"; do
    echo "warning: $warning" >&2
  done
}

# Builds as the invoking user and prints the binary path, so an elevated daemon
# never writes root-owned objects into backend/target. cargo's diagnostics stay
# on the terminal while the artifact messages are parsed for the executable.
build_daemon() {
  local messages
  local binary
  if ! messages=$(cd "$root/backend" && cargo build -q -p anth-panel --message-format=json-render-diagnostics); then
    return 1
  fi
  binary=$(printf '%s\n' "$messages" |
    sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' |
    tail -n 1)
  if [ -z "$binary" ]; then
    echo "error: could not locate the built anth-panel binary" >&2
    return 1
  fi
  printf '%s' "$binary"
}

# The elevated daemon writes its config and state under $run as root; hand them
# back so a later plain `start` can still write there. Best effort, and silent
# when sudo is not available.
restore_run_owner() {
  local uid
  local gid
  [ "$(id -u)" -ne 0 ] || return 0
  [ -d "$run" ] || return 0
  uid=$(id -u)
  gid=$(id -g)
  if [ -z "$(find "$run" ! -user "$uid" -print -quit 2>/dev/null)" ]; then
    return 0
  fi
  sudo -n chown -R "$uid:$gid" "$run" 2>/dev/null || true
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

  # A daemon started elevated belongs to root; the shell needs sudo to signal
  # it. Ask before trying, so a missing password is reported once instead of
  # leaving a half-stopped tree.
  if is_foreign "$pid" && ! sudo_ready; then
    echo "$name: running as another user (pid $pid); rerun as root" >&2
    return 1
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
  if alive "$pid"; then
    echo "$name: still running (pid $pid); rerun as root" >&2
    return 1
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
  local code=0
  local binary
  local launcher=''
  local start_daemon=0

  # The real daemon runs as root, so ask for the password up front: the cached
  # timestamp then covers the ownership fix, the capability probe and the
  # launch itself. The demo backend stays as the caller.
  if [ "$backend" = real ] && ! sudo_ready; then
    exit 2
  fi

  if [ "$backend" = real ]; then
    restore_run_owner
  fi
  start_one backend
  if ! alive "$(read_pid "$backend_pid")"; then
    start_daemon=1
  fi

  if [ "$backend" = real ]; then
    check_network_tools
  fi

  if [ "$start_daemon" = 1 ]; then
    echo "backend: building"
    if ! binary=$(build_daemon); then
      echo "backend: build failed; see the compiler output above" >&2
      return 1
    fi
    if [ "$(id -u)" -ne 0 ]; then
      launcher=" (daemon via sudo from $invoke_dir)"
    fi
    echo "backend: starting on $backend_addr$launcher"
    launch backend "$invoke_dir" "$backend_log" \
      ${daemon_prefix[@]+"${daemon_prefix[@]}"} \
      "$binary" run --addr "$backend_addr" --config "$config"
  fi

  start_one frontend
  if ! alive "$(read_pid "$frontend_pid")"; then
    echo "frontend: starting Vite on $vite_host:$vite_port"
    launch frontend "$root/frontend" "$frontend_log" \
      bun run dev --host "$vite_host" --port "$vite_port"
  fi

  echo "backend: waiting for $health_url"
  if wait_for "$health_url"; then
    echo "backend:  $health_url  (log: $backend_log)"
  else
    echo "backend:  not answering yet; see $backend_log" >&2
    tail -n 10 "$backend_log" >&2 || true
    code=1
  fi

  echo "frontend: waiting for http://127.0.0.1:$vite_port/"
  if wait_for "http://127.0.0.1:$vite_port/"; then
    echo "frontend: http://localhost:$vite_port/  (log: $frontend_log)"
  else
    echo "frontend: not answering yet; see $frontend_log" >&2
    tail -n 10 "$frontend_log" >&2 || true
    code=1
  fi

  echo "stop both with: $0 stop"
  return "$code"
}

stop() {
  local code=0
  stop_one frontend || code=1
  stop_one backend || code=1
  restore_run_owner
  return "$code"
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
