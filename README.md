# Anthracite Panel

A host inspection panel with a Unix-socket daemon at its core.

A single Rust binary can run as a daemon, an HTTP API, or both, and can also
act as a client that queries a running daemon. A Vite + Lit frontend is embedded
in the HTTP server.

## Documentation

| Document | Contents |
| --- | --- |
| [docs/cli.md](docs/cli.md) | Every command, flag, exit code and example |
| [docs/api-endpoints.md](docs/api-endpoints.md) | HTTP/WebSocket routes, request and response bodies, SSE streaming |
| [docs/socket-protocol.md](docs/socket-protocol.md) | Wire format between clients and the daemon |
| [docs/terminal.md](docs/terminal.md) | Interactive terminals, the browser route and current risks |
| [docs/processes.md](docs/processes.md) | Process monitoring, signals, limits and controls |
| [docs/files.md](docs/files.md) | Filesystem root, chunked transfers, write policy and web explorer |

## Layout

```
backend/
  Cargo.toml          workspace root
  anth-proto/         wire protocol: Request/Response + AnthServer/AnthClient
  anth-panel/         the single binary: daemon, API, CLI and web UI server
    www/              built frontend, embedded into the binary at compile time
frontend/             Vite + Lit source, built into backend/anth-panel/www
docs/                 the documentation above
```

`anth-proto` is a separate crate on purpose: it is the contract an alternative
client or daemon can implement against, so it must stay free of panel internals.

## Quick start

The binary embeds the built frontend with `rust-embed` from
`backend/anth-panel/www/`. Populate it to get the web UI; the backend itself
compiles without it. Build the frontend first:

```bash
cd frontend
npm install && npm run build      # or: bun install && bun run build
cp -r dist/. ../backend/anth-panel/www/
```

Then run everything from one process:

```bash
cd backend
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

The workspace pins the `x86_64-unknown-linux-musl` target. If that target is not
installed, pass `--target x86_64-unknown-linux-gnu` to `cargo`.

## Routes at a glance

```bash
curl localhost:8080/healthz
curl localhost:8080/api/host-info
curl -N "localhost:8080/api/host-info/watch?interval=2s"
curl -X POST localhost:8080/api/echo -H 'content-type: application/json' \
  -d '{"message":"hola"}'
```

See [docs/api-endpoints.md](docs/api-endpoints.md) for the full contract,
including error shapes and SSE event fields.

## Development

```bash
cd backend
cargo test --workspace --target x86_64-unknown-linux-gnu
cargo fmt --all --check
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu

cd ../frontend
npm test
npm run build
```

See [AGENTS.md](AGENTS.md) for architecture notes and repository conventions.

## License

Apache-2.0. See [LICENSE](LICENSE).
