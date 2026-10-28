# Anthracite Panel

A host inspection panel built around a Unix-socket daemon, an HTTP API, a CLI
and a web UI. Apache-2.0.

## Status

`v0.3.0` is a rewrite in progress. Phases 0–4 are in place: foundation,
host-info, auth/lifecycle, files and the terminal and process slices. Phase 5 is
code-complete with real-command validation pending: the network inventory, its
guarded interface mutations and the managed nftables firewall are in place.
Phase 6 is code-complete: the Storage inventory, guarded mount/unmount and
partition/format operations, plus the Accounts inventory, guarded create/delete
and password changes, are in place. Accounts success paths were validated on a
real host; storage mutations still need `CAP_SYS_ADMIN`. After Phase 6, plugins
and hardening/release remain.

- Rust workspace: `anth-wire`, `anth-host`, `anth-plugin`, `anth-panel`.
- Length-prefixed JSON wire contract with a mandatory `Hello`/`Welcome`,
  one error type and explicit pagination.
- Pure `/proc` and `os-release` parsers with fixture tests.
- Unix-socket daemon and CLI `host-info`, including the grouped webui-like
  output.
- HTTP API with health, auth, host-info, settings and lifecycle routes; SSE
  watch.
- Minimal auth: `auto`/`generated`/`web` bootstrap, Argon2id, HS256 access
  tokens, rotating refresh tokens, cookies.
- File browsing and editing UI: paginated listing with directories first, text
  and image previews with details and a generic placeholder, a tabbed
  CodeMirror editor with syntax highlighting, a position/encoding status bar,
  saving, uploading from the device, downloading, deletion behind
  `files.write`, the freedesktop.org trash, and in-process archives, over
  socket, HTTP, CLI and the web Files section, with root containment and
  `..`/symlink guards.
- A transfer queue with status, progress, cancellation and completion for
  single or multi-file uploads and downloads, independent of the Files panel
  lifecycle.
- Terminal: one PTY per session with owner/seq/idle limits, a socket read
  contract, a browser WebSocket bridge, a global tabbed xterm window and a
  mobile extended keyboard with gestures. Requires `[terminal] enabled = true`.
- Processes: paginated `/proc` snapshots, an SSE watch feed, allowlisted
  signals guarded by `pid + start_ticks`, allowlisted detached spawns, and a
  flat/tree manager with live CPU/memory in the web UI.
- Network: interface inventory from sysfs and `getifaddrs` with IPv4/IPv6
  addresses, cumulative counters, default routes and client-derived rates, plus
  policy-gated up/down, create, edit, rename and delete through direct `ip`
  argv, over socket, HTTP, SSE, CLI and the Network section.
- Firewall: one managed `inet anthracite` nftables table with `input` and
  `output` chains, allow/deny/edit/delete/flush rules and per-chain default
  policies through direct `nft` argv, over socket, HTTP, CLI and the Firewall
  tab.
- Storage: block devices from `lsblk`, mounts from `/proc/self/mountinfo` with
  best-effort usage, active swap from `/proc/swaps`, a warning-degraded
  snapshot, guarded mount/unmount behind `[volumes] control`, and guarded
  partition create/resize/delete plus `mkfs` behind `[volumes]
  partition/format` and `disk_allow`, over socket, HTTP, CLI and the Storage
  section's Volumes and Partitions tabs.
- Accounts: users and groups from `/etc/passwd`, `/etc/group`, `/etc/shadow`,
  `/etc/gshadow` and `getent`, an explicit lock bit, and guarded create/delete
  and password changes behind `[accounts] control/manage_passwords` with
  system-id and privileged-group opt-ins, over socket, HTTP, CLI and the
  Accounts section's Users and Groups tabs.
- Auth hardening: failures-only login rate limiting with `Retry-After`, session
  listing and revocation.
- Lifecycle: `restart` re-execs and re-reads the config; `reboot` is refused
  unless the daemon runs with `--allow-reboot` and is capped at 300 seconds.
- Settings API and Lit settings page for the policy switches, sessions and
  lifecycle actions.
- Lit shell with the v0.2 layout — sidebar (collapsible to an icon rail),
  drawer, tab strip and FAB — host cards with load/memory/swap bars and
  interval/pause controls, a settings route and English/Spanish
  `lit-localize` setup.
- Config/secrets/state path mapping, atomic writes and comment-preserving
  `config set`; CI for backend and frontend.

The remaining work (network mutations, firewall, volumes/partitions/accounts,
plugins and hardening/release) arrives later. The settings page covers policy
switches, sessions and lifecycle; it is not a full config editor. The plan is
[docs/design/v0.3.0.md](docs/design/v0.3.0.md); the accepted Phase 0 contracts
are [docs/design/phase-0-contracts.md](docs/design/phase-0-contracts.md).

## Toolchain

Rust and Bun live under `/tools` so a recreated environment keeps them:

```bash
. /tools/env.sh
```

- Rust `1.99.1`, target `x86_64-unknown-linux-musl`, `rustfmt`, `clippy`.
- Bun `1.4.4`.
- Alpine C toolchain: `gcc`, `musl-dev`, `make`, `pkgconf`.

## Build and test

```bash
cd backend
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Release builds embed `backend/anth-panel/www/`. Build the shell first:

```bash
cd frontend
bun install
bun run test
bun run typecheck
bun run build
./build-shell.sh

cd ../backend
cargo test --workspace --release
```

Debug builds default to relative paths (`./config.toml`, `./anth.sock`,
`./state`) and a development profile that turns every policy switch on, including
the sensitive ones: terminal, process control and spawn, network management, the
firewall, volume mount/partition/format, account management with passwords,
system and privileged groups, unsigned plugin installs, file writes and trash.
Release builds default to `/etc/anth/config.toml` and `/var/lib/anth` with every
switch closed.

`volumes.disk_allow` stays empty in the development profile on purpose: it lists
the devices an operator is willing to repartition, so the panel never guesses.
Point it at a throwaway disk (or a loop device) before exercising partition
edits; the boot disk remains protected even if it is listed.

Run the panel in development:

```bash
./scripts/dev.sh start      # backend on :8080 + Vite on :5173
./scripts/dev.sh status
./scripts/dev.sh stop
```

The script runs both processes in parallel and keeps the debug config, logs and
PIDs under `/tmp/anthracite-panel/dev`. Vite listens on `0.0.0.0:5173`, so the
UI is reachable from another device on the network; the Vite proxy forwards
`/api` and `/healthz` to `127.0.0.1:8080`. `ANTH_ADDR`, `ANTH_VITE_HOST` and
`ANTH_VITE_PORT` override the defaults.

Phones drop the Vite HMR socket while the native file picker is open, and Vite
reloads the page on return, losing the selection. The script therefore starts
Vite with HMR off; set `ANTH_HMR=on` for fast refresh on desktop. The built
shell on `:8080` has no HMR either.

```bash
ANTH_HMR=on ./scripts/dev.sh restart   # desktop fast refresh
```

Network mutations and the firewall need a daemon with `CAP_NET_ADMIN`, so
`./scripts/dev.sh start` builds the binary as your user and then runs it through
`sudo`, with the directory you invoked the script in as the working directory.
Only the daemon is elevated: `cargo build` and Vite keep your account, so
`backend/target`, `frontend/node_modules` and the run directory keep their
owner. `stop` and `restart` signal the root daemon through `sudo` as well. This
is a development convenience, not a privilege boundary, and it cannot add a
capability the container's bounding set excludes. If `sudo` needs a password,
run it from a terminal or configure NOPASSWD; a non-interactive caller gets an
error instead of a silent wait.

Inside an OpenCode session the same commands are available as `/dev-start`,
`/dev-stop`, `/dev-status` and `/dev-restart` (from `.opencode/commands/`).

The first debug start uses the web bootstrap by default: open the UI, or
`POST /api/auth/register` with `{"password":"at least 12 characters"}`. A release
build defaults to printing a generated password once on stderr.

CLI:

```bash
cd backend
cargo run -p anth-panel -- config set version 1 --config ./config.toml
cargo run -p anth-panel -- config validate --config ./config.toml
cargo run -p anth-panel -- host-info --config ./config.toml
cargo run -p anth-panel -- host-info --watch 2s --config ./config.toml
```

## Documentation

| Document | Contents |
| --- | --- |
| [docs/README.md](docs/README.md) | Documentation index |
| [docs/cli.md](docs/cli.md) | Commands, flags, output and exit codes |
| [docs/api-endpoints.md](docs/api-endpoints.md) | HTTP/SSE routes, auth and error shape |
| [docs/socket-protocol.md](docs/socket-protocol.md) | Framing, handshake and messages |
| [docs/design/v0.3.0.md](docs/design/v0.3.0.md) | Architecture draft and phases |
| [docs/design/phase-0-contracts.md](docs/design/phase-0-contracts.md) | Wire, config/state, bootstrap, HTTP/CLI, logging and git contracts |
| [docs/design/reference-v0.1-v0.2.md](docs/design/reference-v0.1-v0.2.md) | Architecture of the two previous trees |
| [docs/design/protocol-changelog.md](docs/design/protocol-changelog.md) | Wire versions and the pre-1.0.0 compatibility scaffold |
