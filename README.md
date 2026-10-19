# Anthracite Panel

A host inspection panel with a Unix-socket daemon at its core.

A single Rust binary can run as a daemon, an HTTP API, or both, and can also
act as a client that queries a running daemon. A Vite + Lit frontend is embedded
in the HTTP server.

## Documentation

| Document | Contents |
| --- | --- |
| [docs/cli.md](docs/cli.md) | Every command, flag, exit code and example |
| [docs/api-endpoints.md](docs/api-endpoints.md) | HTTP routes, request and response bodies, SSE streaming |
| [docs/socket-protocol.md](docs/socket-protocol.md) | Wire format between clients and the daemon |

## Layout

```
backend/
  Cargo.toml          workspace root
  anth-proto/         wire protocol: Request/Response + AnthServer/AnthClient
  anth-panel/         the single binary: daemon, API, CLI and web UI server
    webui/            built frontend, embedded into the API binary
frontend/             Vite + Lit source
docs/                 the documentation above
```

`anth-proto` is a separate crate on purpose: it is the contract an alternative
client or daemon can implement against, so it must stay free of panel internals.

## Quick start

```bash
cd backend

# Build the frontend first; anth-panel embeds it at compile time.
(cd anth-panel/webui && npm install && npm run build)

# One process serving the daemon and the API on 127.0.0.1:8080
cargo run -p anth-panel -- run --addr 127.0.0.1:8080

# In another shell
cargo run -p anth-panel -- host-info
cargo run -p anth-panel -- host-info --watch 2s
```

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
```

See [AGENTS.md](AGENTS.md) for architecture notes and repository conventions.

## License

Apache-2.0. See [LICENSE](LICENSE).
