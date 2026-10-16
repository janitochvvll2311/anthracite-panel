# AGENTS.md

Repository-specific knowledge for agents working on Anthracite Panel.

## What this is

Rust workspace for the "Anthracite Panel": a Unix-socket daemon (`anthd`), a thin
CLI client (`anth-cli`), and an HTTP panel with an embedded web UI (`anth-panel`).
License is Apache-2.0. No README exists yet.

## Layout

```
Cargo.toml                  # workspace root, resolver = "2"
.cargo/config.toml          # default build target = x86_64-unknown-linux-musl
crates/
  anth-proto/               # wire protocol: Request/Response enums + AnthServer/AnthClient
  anthd/                    # daemon: binds the Unix socket, dispatches requests
  anth-conf/                # Config struct, TOML load/save, default paths
  anth-cli/                 # CLI client: builds a Request, prints the Response
  anth-panel/               # axum HTTP API + rust-embed static SPA
    webui/                  # Lit + Vite frontend (TypeScript)
docker/entrypoint.sh        # starts anthd then anth-panel
Dockerfile                  # multi-stage alpine/musl build
```

Dependencies flow: `anth-proto` and `anth-conf` are leaves; `anthd`, `anth-cli`
and `anth-panel` depend on both.

## Build and test

The workspace pins `x86_64-unknown-linux-musl` in `.cargo/config.toml`. A bare
`cargo build` therefore needs that target installed. When it is not installed,
override the target explicitly:

```bash
cargo build --workspace --target x86_64-unknown-linux-gnu
cargo test  --workspace --target x86_64-unknown-linux-gnu
cargo fmt --check
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu
```

There is no CI configuration, no `rustfmt.toml` and no `clippy.toml` — defaults apply.

### anth-panel requires the web UI build first

`crates/anth-panel/src/lib.rs` embeds `webui/dist` with `#[derive(RustEmbed)]`.
`webui/dist` is gitignored and is NOT committed, so a fresh clone cannot compile
`anth-panel` until the frontend is built:

```bash
cd crates/anth-panel/webui && npm install && npm run build   # produces dist/
```

A missing `dist/` surfaces as `no associated function named 'get' found for struct 'Asset'`.
The webui has a `bun.lock`, but `npm` works fine and `bun` is not required.

## Running locally

```bash
cargo run -p anthd                                                   # daemon, reads /etc/anth/config.toml
cargo run -p anth-cli -- echo "hola"                                 # needs a running daemon
ANTH_PANEL_ADDR=0.0.0.0:8080 cargo run -p anth-panel
```

## Architecture notes

- **Transport**: newline-delimited JSON over a Unix socket (`tokio_util` `LinesCodec`
  wrapped in `Framed`), exposed as `AnthServer`/`AnthClient` in `anth-proto`.
- **Protocol**: `Request` and `Response` are serde-tagged enums using
  `#[serde(tag = "type", content = "data")]`. Today the only request is
  `Request::Echo { message }`; responses are `Response::Ok { message }` or
  `Response::Error { detail }`. Add a new operation by extending both enums plus
  the `match` arms in `anthd::handle_request` and `anth_cli::request_for`.
- **Config**: `anth-conf::Config` (currently just `socket_path`) is TOML with
  `#[serde(default, deny_unknown_fields)]`. Defaults: config at
  `/etc/anth/config.toml`, socket at `/var/tmp/anth.sock`. `anthd` writes a default
  config when loading fails.
- **Panel routes**: `GET /healthz`, `POST /command` (proxies a JSON `Request` to the
  daemon over the socket), and a fallback static handler that serves the embedded SPA
  and falls back to `index.html`. Table tests exercise the router through
  `tower::ServiceExt::oneshot`.
- **Env vars**: `ANTH_PANEL_ADDR` (panel bind address, default `0.0.0.0:8080`),
  `ANTH_CONFIG` (config path, used by the Docker entrypoint), `RUST_LOG`.

## Code style

- Errors: library functions return `Result<T, Box<dyn Error + Send + Sync>>`;
  `anth-conf` file operations return `Result<_, String>` with the path formatted into
  the message. Libraries log failures with `eprintln!`; `main` returns `ExitCode`.
- Tests: one integration file per crate at `crates/<name>/tests/tests.rs`. Tests are
  plain `#[test]` functions with `#[tokio::test]` for async. Socket-based tests bind a
  unique path in `std::env::temp_dir()` (pid + nanos) instead of a shared fixed path.
  Prefer exercising real code paths — `anthd` tests spin up a real `UnixListener`, and
  `anth-panel` tests use a real in-process echo server.
- Crate editions are inconsistent: `anth-conf` uses edition 2024, all other crates
  use 2021. Match the crate you are editing rather than normalizing in passing.
- Commit messages mix conventional prefixes (`feat:`, `chore:`) with plain
  capitalized sentences. Keep whichever style the surrounding history uses.
