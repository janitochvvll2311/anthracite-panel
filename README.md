# Anthracite Panel

A host inspection panel with a Unix-socket daemon at its core.

A single Rust binary can run as a daemon, an HTTP API, or both, and can also
act as a client that queries a running daemon. A Vite + Lit frontend is embedded
in the HTTP server.

## Two versions live here

`v0.1.0` shipped and is tagged. `v0.2.0` is a rewrite in progress, and its code
is what sits at the repository root.

```
Cargo.toml            v0.2.0 workspace root
anth-wire/            wire protocol: length-prefixed frames, one error type
anth-host/            host inspection: /proc and os-release parsing, std only
anth-paneld/          daemon, CLI client and (later) the HTTP API
.cargo/config.toml    pins x86_64-unknown-linux-musl for this workspace
docs/design/          v0.2.0 design notes

v1/                   the frozen v0.1.0 tree, untouched and still buildable
  backend/            workspace root: anth-proto + anth-panel
  frontend/           Vite + Lit source
  plugins/            the WireGuard reference plugin
  docs/               the user documentation for v0.1.0
  Dockerfile          v1 image, built with v1/ as the build context
```

`v1/` is a complete, self-contained v0.1.0 checkout: build it from `v1/backend`
and read its docs in `v1/docs/`. Nothing at the root imports from it, and phase E
of the rewrite deletes it once the port is finished.

The v0.2.0 tree does **not** ship yet. It has one feature (`host-info`), so treat
the v1 documentation as the description of the working product until the port
completes. [docs/design/v0.2.0.md](docs/design/v0.2.0.md) records why the rewrite
is happening, with file evidence for each lesson, and what is done so far.

## Documentation

User-facing documentation for the shipped version:

| Document | Contents |
| --- | --- |
| [v1/docs/cli.md](v1/docs/cli.md) | Every command, flag, exit code and example |
| [v1/docs/api-endpoints.md](v1/docs/api-endpoints.md) | HTTP/WebSocket routes, request and response bodies, SSE streaming |
| [v1/docs/socket-protocol.md](v1/docs/socket-protocol.md) | Wire format between clients and the daemon |
| [v1/docs/terminal.md](v1/docs/terminal.md) | Interactive terminals, the browser route and current risks |
| [v1/docs/processes.md](v1/docs/processes.md) | Process monitoring, signals, limits and controls |
| [v1/docs/network.md](v1/docs/network.md) | Network interfaces, traffic counters and up/down policy |
| [v1/docs/firewall.md](v1/docs/firewall.md) | Managed nftables table, rules and container capabilities |
| [v1/docs/volumes.md](v1/docs/volumes.md) | Storage inventory, mount and unmount policy, privileges |
| [v1/docs/accounts.md](v1/docs/accounts.md) | Local users and groups, guard matrix, privileges and container limits |
| [v1/docs/files.md](v1/docs/files.md) | Filesystem root, chunked transfers, write policy and web explorer |

Design notes: [v1/docs/design/](v1/docs/design/) for the shipped version, and
[docs/design/](docs/design/) for the rewrite.

## Quick start

### v0.1.0 (works)

The binary embeds the built frontend with `rust-embed` from
`v1/backend/anth-panel/www/`. Populate it to get the web UI; the backend itself
compiles without it. Build the frontend first:

```bash
cd v1/frontend
bun install && bun run build
cp -r dist/. ../backend/anth-panel/www/
```

Then run everything from one process:

```bash
cd v1/backend
cargo run -p anth-panel -- run --addr 127.0.0.1:8080
```

In another shell, using the same binary:

```bash
anth-panel host-info
anth-panel host-info --watch 2s
```

`www/` is gitignored apart from a placeholder. Without the build the binary
still compiles and the API works, but `GET /` answers `404`: there is no UI to
serve. Build the frontend once to get the panel.

### v0.2.0 (one feature so far)

From the repository root:

```bash
cargo run -p anth-paneld -- daemon &
cargo run -p anth-paneld -- host-info
```

The daemon speaks the new length-prefixed protocol; it is not wire-compatible
with the v0.1.0 client.

## Toolchain

Two choices are fixed, not preferences:

- **Rust targets `x86_64-unknown-linux-musl`.** Each workspace pins it in its own
  `.cargo/config.toml` (root and `v1/backend/`), so build with a bare
  `cargo build` and the result is static. Install the target once with
  `rustup target add x86_64-unknown-linux-musl` and `musl-tools` for the C
  dependencies. Do not pass `--target x86_64-unknown-linux-gnu`.
- **`bun` is the only package manager.** `npm`, `npx` and `corepack` are not
  installed and `bunx` is unavailable; use `bun run <script>`.

The v0.1.0 plugin tests additionally need
`rustup target add wasm32-unknown-unknown`.

## Development

The v0.2.0 workspace, from the repository root:

```bash
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

Those three are what [.github/workflows/v2.yml](.github/workflows/v2.yml) runs,
so a local pass means a CI pass.

The v0.1.0 tree, from `v1/backend`:

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets
```

And its frontend, from `v1/frontend`:

```bash
bun run test
bun run build
```

See [AGENTS.md](AGENTS.md) for architecture notes and repository conventions.
Debug builds of v0.1.0 generate a missing config with global sudo, process
control, network control, firewall control, volume control and file writes
enabled so the full UI is testable. Sudo runs `ip link set`, `nft`, `mount` and
`umount` through `sudo -n`; each falls back to its direct path. Release builds
keep the controls disabled.

## License

Apache-2.0. See [LICENSE](LICENSE).
