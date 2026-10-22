# AGENTS.md

Repository-specific knowledge for agents working on Anthracite Panel.

## What this is

A host inspection panel: a Unix-socket daemon, an HTTP API, a CLI client and a
web UI. Apache-2.0.

Two trees live here:

- **The repository root is the v0.2.0 rewrite**, in progress. This file describes it.
- **`v1/` is the frozen v0.1.0 tree**, tagged and still buildable, with its own
  `v1/AGENTS.md` and `v1/docs/`. Do not edit `v1/` except to keep it building:
  it is the reference the rewrite is measured against. Phase F deletes it.

`docs/design/v0.2.0.md` is the plan of record. Read it before porting a feature:
it names the four structural problems the rewrite exists to fix, with the file
and line evidence behind each one.

## Layout

```
backend/                    # Rust workspace, resolver = "2", version 0.2.0
  Cargo.toml
  Cargo.lock
  .cargo/config.toml        # pins x86_64-unknown-linux-musl for this workspace
  anth-wire/                # wire protocol only; no panel code
    src/lib.rs              #   re-exports, PROTOCOL_VERSION
    src/frame.rs            #   length-prefixed framing, MAX_FRAME_LEN
    src/message.rs          #   Request, Response
    src/error.rs            #   WireError, ErrorKind: status + exit code
  anth-host/                # host inspection, standard library only
    src/lib.rs              #   collect() and collect_from(root)
    src/parse.rs            #   pure parsers over /proc and os-release contents
  anth-paneld/              # the binary: daemon, HTTP API and CLI client
    src/main.rs             #   CLI parsing, exit-code policy, serve_all
    src/dispatch.rs         #   the single request match
    src/api.rs              #   axum router over dispatch; ApiError -> status
    src/socket.rs           #   Unix listener, connection loop, client
    src/render.rs           #   bordered table for the CLI

frontend/                   # Bun workspace, bun.lock committed
  packages/shared/          # @anth/shared: wire types and formatting, no transport
  packages/app/             # @anth/app: the shell, Vite build, dist/ gitignored
  bunfig.toml               # test preload: happy-dom for component tests

plugins/                    # one directory per plugin
  hello/                    # the example: manifest, backend/, frontend/, package.sh

docs/design/v0.2.0.md       # why the rewrite exists, phases, what is done
v1/                         # frozen v0.1.0: backend, frontend, plugins, docs
```

Crates are separated by authority, not size: `anth-wire` is the contract an
alternative client could implement against, so it must never depend on panel
internals, and `anth-host` is pure parsing so it can be tested without a daemon.

`backend/`, `frontend/` and each plugin are **independent projects**, not one
build. `backend/Cargo.toml` does not list the plugin crates, and `frontend/`
does not install plugin dependencies. That is deliberate: a plugin ships to a
third party, so it must build with nothing but its own directory.

## Build and test

Run from `backend/`. The musl target is pinned by `backend/.cargo/config.toml`,
so a bare `cargo build` produces the same static binary production ships. Never
pass `--target x86_64-unknown-linux-gnu`.

```bash
cd backend
cargo build  --workspace
cargo test   --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

From `frontend/`:

```bash
bun install            # once, or after a package.json change
bun run typecheck
bun test
bun run build          # typechecks, then writes packages/app/dist
```

From `plugins/hello/`, which CI also runs:

```bash
(cd backend && cargo test)          # needs no wasm target
(cd frontend && bun test)
./package.sh                        # wasm + bun bundle + manifest digest
```

`.github/workflows/v2.yml` runs all of the above in a `rust`, a `web` and a
`plugin` job, so a local pass means a CI pass. Clippy runs with `-D warnings`: a
redundant closure is a build failure, not a suggestion.

Prerequisites, once per machine:

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools     # musl-gcc, for the C dependencies
```

## Design rules

These are the four lessons the rewrite exists for. A change that violates one
needs a stated reason in the commit message.

1. **No size-fitting passes.** v0.1.0 capped a JSON line at 1 MiB and answered
   with five truncation passes (`process/fit.rs`, `network`, `firewall`,
   `volumes`, `accounts`) that silently shortened lists and set `truncated`
   flags. A frame is bounded at `MAX_FRAME_LEN` and exceeding it is a
   `WireError`, never a shortened answer. If a response is too large, split it
   with pagination at the call site, not by dropping rows mid-serialization.
2. **One error type.** `WireError` with seven `ErrorKind`s replaces the six
   per-feature `*ErrorKind` enums. It derives **both** the HTTP status and the
   CLI exit code in `error.rs`. Do not hand-write a status or an exit code at a
   call site: `serve` used to hardcode `ExitCode::from(1)`, and a test now pins
   that it derives from the kind.
3. **One dispatch site.** A new request adds an arm to `anth-paneld/src/dispatch.rs`
   and nowhere else. v0.1.0 repeated each variant across the handler, the client
   and the daemon matches, and nothing tied them together at compile time. The
   HTTP route in `api.rs` calls that same `dispatch`, so the two transports
   cannot answer differently for the same command.
4. **No vacuous tests.** A test must be able to fail. Fixtures that read a file
   panic when it is missing rather than returning an empty value that makes
   every assertion trivially pass. Prefer a real code path — a real listener, a
   real `/proc` fixture tree — over a mock.

## Environment

- **Rust targets musl, always.** Never build this project for gnu.
- **`bun` is the only package manager.** `npm`, `npx`, `corepack` and `bunx` do
  not exist here. Run scripts as `bun run <script>`. The v0.2.0 frontend is
  `frontend/`; the frozen v0.1.0 one is `v1/frontend/`.
- **The wasm target is only needed for the plugin**, not for `backend/`:
  `rustup target add wasm32-unknown-unknown`. The plugin's tests run natively.
- Integration tests live at `anth-*/tests/<module>.rs`. Socket-based tests bind a
  unique path in `std::env::temp_dir()` (pid + nanos) and remove it on drop, so
  two tests never share a socket.
- `ANTH_SOCKET` overrides the daemon and client socket path, which is what the
  tests and the demo use; the default is `/var/tmp/anth.sock`, unchanged from
  v0.1.0 so an upgraded host needs no migration.

## Code style

- Libraries return `WireError` for protocol and I/O failures; a pure parser
  returns `Option` or a defaulted value and lets `collect_from` degrade a field
  to `unknown` rather than failing the whole snapshot.
- `anth-host` uses the standard library only, and no `unsafe`. It reads `/proc`
  and `/etc/os-release`; it must not need a daemon or a mount namespace to test.
- Module layout is by concern. A file that grows past a few hundred lines
  usually holds two jobs: split it into a directory with `mod.rs` and one file
  per job, and move `#[cfg(test)]` blocks to `tests.rs` beside it.
- Commit messages mix conventional prefixes (`feat:`, `docs:`) with plain
  capitalized sentences; keep whichever style the surrounding history uses.
