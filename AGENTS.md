# AGENTS.md

Repository-specific knowledge for agents working on Anthracite Panel.

## What this is

Two sibling projects: a Rust backend and a Vite/Lit frontend. The backend ships a
single binary, `anth-panel`, that serves the Unix-socket daemon, the CLI client and
the HTTP panel with an embedded web UI. License is Apache-2.0. No README exists yet.

## Layout

```
backend/
  Cargo.toml                # workspace root, resolver = "2"
  Cargo.lock
  .cargo/config.toml        # default build target = x86_64-unknown-linux-musl
  anth-proto/               # wire protocol only: Request/Response + AnthServer/AnthClient
  anth-panel/               # everything else, one binary
    src/
      main.rs               #   subcommand dispatch: daemon / api / run / echo / host-info
      lib.rs                #   module re-exports
      cli.rs                #   clap definitions
      config.rs             #   Config struct, TOML load/save, default paths
      daemon.rs             #   Unix socket listener and request dispatch
      client.rs             #   CLI client: builds a Request, prints the Response
      api.rs                #   axum HTTP API + rust-embed static SPA
      app.rs                #   AppState shared by the routes
      hostinfo.rs           #   host inspection from /proc and /etc/os-release
      render.rs             #   bordered table renderer for the CLI
      stream.rs             #   repeat a request: interval parsing + frame output
      poll.rs               #   repeat an async read on a period (shared timing)
      sse.rs                #   SSE framing: named events, ids, keep-alive
    www/                    #   rust-embed folder; holds only .gitkeep in the repo
    tests/                  #   api.rs, client.rs, config.rs, daemon.rs, hostinfo.rs, render.rs,
                            #   stream.rs, sse.rs
frontend/                   # Vite + Lit + TypeScript, package name `anth-panel`
  vite.config.ts            # proxies /healthz and /command to the backend in dev
  src/  public/  index.html
docker/entrypoint.sh        # starts `anth-panel run`
Dockerfile                  # bun frontend stage, musl backend stage, alpine runtime
```

`anth-proto` is deliberately separate so alternative or custom tooling can implement
the protocol without depending on the panel. `anth-panel` depends on it.

## Build and test

Run these from `backend/`. The workspace pins `x86_64-unknown-linux-musl` in
`.cargo/config.toml`, so a bare `cargo build` needs that target installed. When it is
not installed, override the target explicitly:

```bash
cargo build  --workspace --target x86_64-unknown-linux-gnu
cargo test   --workspace --target x86_64-unknown-linux-gnu
cargo fmt --check
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu
```

There is no CI configuration, no `rustfmt.toml` and no `clippy.toml` — defaults apply.
`cargo clippy` reports two `needless_borrows_for_generic_args` warnings in
`anth_panel::daemon::serve`; they predate the current layout and are unrelated to it.

### The backend compiles without a frontend build

`anth-panel/src/api.rs` embeds `www/` with `#[derive(RustEmbed)]`. In development `www/`
holds only a `.gitkeep` and Vite serves the UI, proxying the API routes itself — see
`frontend/vite.config.ts`. To exercise the embedded UI, build the frontend and copy its
output into `www/` first:

```bash
cd frontend && npm install && npm run build && cp -r dist/. ../backend/anth-panel/www/
```

Generated files under `www/` are gitignored except the placeholder. A missing folder
surfaces as `no associated function named 'get' found for struct 'Asset'`. The frontend
has a `bun.lock`, but `npm` works fine and `bun` is not required.

## Running locally

Development uses two processes; the frontend proxies to the backend:

```bash
cd backend  && cargo run -p anth-panel -- run --addr 127.0.0.1:8080   # daemon + API
cd frontend && npm run dev                                            # UI on :5173
```

Single-purpose invocations:

```bash
cargo run -p anth-panel -- daemon         # daemon only, reads /etc/anth/config.toml
cargo run -p anth-panel -- api            # HTTP API only; needs a daemon
cargo run -p anth-panel -- echo "hola"    # CLI client; needs a running daemon
cargo run -p anth-panel -- host-info      # host snapshot as a table; needs a daemon
cargo run -p anth-panel -- host-info --watch          # same, refreshed every 2s
cargo run -p anth-panel -- host-info --watch 500ms    # same, every 500ms
```

`host-info --watch [INTERVAL]` repeats the read until Ctrl-C. The interval takes an
`ms`/`s`/`m`/`h` suffix, defaults to `2s` when the flag is bare, and is rejected below
`10ms` so a typo cannot turn into a busy loop. On a terminal each frame overwrites the
previous one; when redirected, frames are separated by a blank line. One connection is
kept open for the whole stream. Ctrl-C exits `0`; a dropped daemon exits `1`.

`host-info` prints the operating system and resources of the host the daemon runs on
(hostname, distro, kernel, architecture, CPU, cores, uptime, load average, memory and
swap) as a bordered table. It deliberately excludes per-process inspection.

## Architecture notes

- **Transport**: newline-delimited JSON over a Unix socket (`tokio_util` `LinesCodec`
  wrapped in `Framed`), exposed as `AnthServer`/`AnthClient` in `anth-proto`.
- **Protocol**: `Request` and `Response` are serde-tagged enums using
  `#[serde(tag = "type", content = "data")]`. Requests are `Request::Echo { message }`
  and the payload-less `Request::HostInfo`; responses are `Response::Ok { message }`,
  `Response::Error { detail }` or the boxed `Response::HostInfo`. Add a new operation by
  extending both enums plus the `match` arms in `anth_panel::daemon::handle_request` and
  `anth_panel::client::request_for`, and a subcommand in `cli.rs`.
- **Host inspection**: `anth_panel::hostinfo` reads `/proc` and `/etc/os-release` with the
  standard library only — no `sysinfo`-style dependency and no `unsafe`, which keeps the
  static musl build trivial. `collect_from` takes a filesystem root so tests can exercise
  the parsing against fixture files; the `parse_*` helpers are pure functions over file
  contents. Unreadable fields degrade to `unknown`/zero rather than failing the request.
- **Streaming**: `anth_panel::poll::stream` owns *when* a read repeats — one Tokio
  interval per stream, `MissedTickBehavior::Delay`, and a wait that is cancelled when
  the stream is dropped. It knows nothing about transport or formatting, so the CLI
  and the SSE endpoint share it. `sse::SseStream` owns *how* values become SSE frames
  (event name, ids, retry hint, keep-alive, error event) and takes any
  `Stream<Item = Result<T, E>>`; a new endpoint reuses it by naming its event.
  `anth_panel::stream::follow` is request-agnostic — it takes the socket
  path, a `Request`, an `Option<Duration>` period and a `FnMut(&Response) -> String`
  renderer, so a new streaming subcommand only has to answer `Some(period)` in
  `watch_interval`. The wait is a Tokio interval with `MissedTickBehavior::Delay`, which
  yields the thread instead of spinning and avoids a catch-up burst after a slow read.
  `Frames` owns the redraw-vs-blank-line decision and is generic over `Write`, which is
  how the frame layout is tested without a TTY.
- **Table rendering**: `anth_panel::render::Table` draws a bordered box (top, bottom, a
  rule under the header and optional separators between groups) and pads columns by
  character count so multi-byte text stays aligned. `render_host_info` maps a `HostInfo`
  onto it. It is a local renderer, not a dependency.
- **Config**: `anth_panel::config::Config` (currently just `socket_path`) is TOML with
  `#[serde(default, deny_unknown_fields)]`. Defaults: config at `/etc/anth/config.toml`,
  socket at `/var/tmp/anth.sock`. The server subcommands load it through
  `Config::load_or_create`, which writes a default file when the path cannot be read,
  so a first run bootstraps `/etc/anth/config.toml`; the client subcommand uses
  `Config::load_or_default`, which never writes.
- **Panel routes**: `GET /healthz`, plus one route per command under `/api`:
  `GET /api/host-info`, `GET /api/host-info/watch?interval=2s` (SSE) and
  `POST /api/echo`. The route names the command, so bodies carry the command's
  arguments at the top level (`{"message":"hi"}`) with no `type`/`data` envelope;
  the envelope stays a `anth-proto` concern for socket clients only. A fallback
  static handler that serves the embedded SPA
  and falls back to `index.html`. Table tests exercise the router through
  `tower::ServiceExt::oneshot`.
- **Subcommands**: `daemon`, `api` and `run` share `--addr`, which falls back to
  `ANTH_PANEL_ADDR` and then `0.0.0.0:8080`. `run` binds the HTTP port before starting
  the daemon and aborts it when the API stops, which is what the container uses.
  `--config` is `global = true`, so it is accepted before or after the subcommand.
- **Env vars**: `ANTH_PANEL_ADDR` (panel bind address, default `0.0.0.0:8080`),
  `ANTH_CONFIG` (config path, used by the Docker entrypoint), `RUST_LOG`.

## Code style

- Errors: library functions return `Result<T, Box<dyn Error + Send + Sync>>`;
  `anth_panel::config::Config` file operations return `Result<_, String>` with the path
  formatted into the message. Libraries log failures with `eprintln!`; `main` returns
  `ExitCode`.
- Tests: integration files live at `backend/anth-panel/tests/<module>.rs`, one per module,
  plus `backend/anth-proto/tests/tests.rs`. They are plain `#[test]` functions with
  `#[tokio::test]` for async. Socket-based tests bind a unique path in
  `std::env::temp_dir()` (pid + nanos) instead of a shared fixed path. Prefer exercising
  real code paths — daemon tests spin up a real `UnixListener`, and API tests use a real
  in-process echo server.
- The single crate and its manifest are `anth-panel`, version `0.1.0`, edition 2024.
- Commit messages mix conventional prefixes (`feat:`, `chore:`) with plain
  capitalized sentences. Keep whichever style the surrounding history uses.
