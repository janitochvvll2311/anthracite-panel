# Anthracite Panel

A host inspection panel built around a Unix-socket daemon, an HTTP API, a CLI
and a web UI. Apache-2.0.

## Status

`v0.3.0` is a rewrite in progress. The Phase 1 host-info slice and Phase 2
auth/lifecycle slice are in place:

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
- File browsing and editing UI: paginated listing, text and image previews with
  details and a generic placeholder, a tabbed CodeMirror editor with syntax
  highlighting, a position/encoding status bar, saving, uploading from the
  device, downloading and deletion behind `files.write`, over socket, HTTP, CLI
  and the web Files section, with root containment and `..`/symlink guards.
- A transfer queue with status, progress, cancellation and completion for
  uploads and downloads, independent of the Files panel lifecycle.
- Auth hardening: failures-only login rate limiting with `Retry-After`, session
  listing and revocation.
- Lifecycle: `restart` re-execs and re-reads the config; `reboot` is refused
  unless the daemon runs with `--allow-reboot` and is capped at 300 seconds.
- Settings API and Lit settings page for the policy switches, sessions and
  lifecycle actions.
- Lit shell with the v0.2 layout — sidebar, drawer, tab strip and FAB — host
  cards with load/memory/swap bars and interval/pause controls, a settings route
  and English/Spanish `lit-localize` setup.
- Config/secrets/state path mapping, atomic writes and comment-preserving
  `config set`; CI for backend and frontend.

The remaining features (processes, network, firewall, volumes, accounts,
terminal, plugins, settings) arrive in later phases. The plan is
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
`./state`). Release builds default to `/etc/anth/config.toml` and
`/var/lib/anth`.

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
