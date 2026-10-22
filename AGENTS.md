# AGENTS.md

Repository-specific knowledge for agents working on Anthracite Panel.

## What this is

A host inspection panel: a Unix-socket daemon, an HTTP API, a CLI client and a
web UI. Apache-2.0.

Two trees live here:

- **The repository root is the v0.2.0 rewrite**, in progress. This file describes it.
- **`v1/` is the frozen v0.1.0 tree**, tagged and still buildable, with its own
  `v1/AGENTS.md` and `v1/docs/`. Do not edit `v1/` except to keep it building:
  it is the reference the rewrite is measured against. Phase E deletes it.

`docs/design/v0.2.0.md` is the plan of record. Read it before porting a feature:
it names the four structural problems the rewrite exists to fix, with the file
and line evidence behind each one.

## Layout

```
Cargo.toml                  # v0.2.0 workspace root, resolver = "2"
Cargo.lock
.cargo/config.toml          # pins x86_64-unknown-linux-musl for this workspace
anth-wire/                  # wire protocol only; no panel code
  src/lib.rs                #   re-exports, PROTOCOL_VERSION
  src/frame.rs              #   length-prefixed framing, MAX_FRAME_LEN
  src/message.rs            #   Request, Response
  src/error.rs              #   WireError, ErrorKind: status + exit code
anth-host/                  # host inspection, standard library only
  src/lib.rs                #   collect() and collect_from(root)
  src/parse.rs              #   pure parsers over /proc and os-release contents
anth-paneld/                # the binary: daemon and CLI client
  src/main.rs               #   CLI parsing, exit-code policy
  src/dispatch.rs           #   the single request match
  src/socket.rs             #   Unix listener, connection loop, client
  src/render.rs             #   bordered table for the CLI
docs/design/v0.2.0.md       # why the rewrite exists, phases, what is done
v1/                         # frozen v0.1.0: backend, frontend, plugins, docs
```

Crates are separated by authority, not size: `anth-wire` is the contract an
alternative client could implement against, so it must never depend on panel
internals, and `anth-host` is pure parsing so it can be tested without a daemon.

## Build and test

Run from the repository root. The musl target is pinned by `.cargo/config.toml`,
so a bare `cargo build` produces the same static binary production ships. Never
pass `--target x86_64-unknown-linux-gnu`.

```bash
cargo build  --workspace
cargo test   --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

`.github/workflows/v2.yml` runs the last three, so a local pass means a CI pass.
Clippy runs with `-D warnings`: a redundant closure is a build failure, not a
suggestion.

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
   and the daemon matches, and nothing tied them together at compile time.
4. **No vacuous tests.** A test must be able to fail. Fixtures that read a file
   panic when it is missing rather than returning an empty value that makes
   every assertion trivially pass. Prefer a real code path — a real listener, a
   real `/proc` fixture tree — over a mock.

## Environment

- **Rust targets musl, always.** Never build this project for gnu.
- **`bun` is the only package manager.** `npm`, `npx`, `corepack` and `bunx` do
  not exist here. Run scripts as `bun run <script>`. There is no frontend at the
  root yet; the v1 frontend is under `v1/frontend`.
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
