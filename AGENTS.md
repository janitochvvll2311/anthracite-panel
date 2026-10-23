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
  anth-plugin/              # the plugin contract a guest links: no_std
    src/lib.rs              #   re-exports
    src/abi.rs              #   anth_plugin! macro: one export per transport
    src/message.rs          #   Action, Request, Status, Response: no transport
    src/transport.rs        #   HttpRequest, CliRequest (line), Method
    src/plugin.rs           #   Plugin trait: actions + three mappers
    src/exec.rs             #   Executor trait: the capability seam
  anth-paneld/              # the binary: daemon, HTTP API and CLI client
    src/main.rs             #   CLI parsing, exit-code policy, serve_all
    src/dispatch.rs         #   the single request match
    src/api.rs              #   axum router over dispatch; ApiError -> status
    src/socket.rs           #   Unix listener, connection loop, client
    src/render.rs           #   bordered table for the CLI

frontend/                   # Bun workspace, bun.lock committed
  anth-wire/                # @anth/wire: what crosses the boundary, no panel code
    src/error.ts            #   ErrorKind + the one status map, mirrors anth-wire
    src/message.ts          #   payload interfaces, no socket envelope
    src/exchange.ts         #   one JSON exchange over fetch; WireError
  anth-webui/               # @anth/webui: the shell, Vite build, dist/ gitignored
    src/wire.ts             #   presentation only: formatKib, formatUptime
  anth-plugin/              # @anth/plugin: anchors, widget base, registration
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
bun run typecheck      # wire, webui, then plugin
bun test               # happy-dom preloaded from bunfig.toml
bun run build          # typechecks, then writes anth-webui/dist
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

## The plugin contract

Two libraries hold the contract a plugin must satisfy, one per half, and a
plugin is expected to contain behaviour only.

- **`backend/anth-plugin`** is what a guest links. It is `no_std` (a wasm guest
  has no `std`) and owns four things:
  - `abi.rs` — `ABI_VERSION = 2` and the `anth_plugin!` macro that generates the
    six exports the host checks by name and signature: one entry per transport
    (`anth_http`, `anth_cli`, `anth_direct`) plus the memory contract
    (`anth_abi_version`, `anth_alloc`, `anth_free`). The two imports it provides
    are `anth_exec` and `anth_log`, gated to `wasm32`; off wasm `HostExecutor`
    refuses every capability by name, so a plugin crate still builds and tests
    where no daemon exists. Version 2 replaced the single `anth_handle`, so an
    `abi = 1` bundle has no `anth_http` to call and is refused rather than
    silently broken.
  - `message.rs` — the plugin's own vocabulary, and it names no transport. Named
    after `anth-wire`'s `message` module and private for the same reason: it holds
    both directions of one exchange, and the types are re-exported so a plugin
    writes `anth_plugin::Request` rather than a module path.
    `Action(&'static str)` is a validated const, never caller input: a name
    arriving at run time can only become an `Action` by being found in
    `Plugin::actions()`, so a payload cannot invent an operation.
    `Request { action, value }` is what every pipe produces.
    `Status` is an enum, not a `u16`: the plugin names an outcome, `code()` turns
    it into an HTTP status and `exit_code()` into a CLI exit code (`BadRequest`
    is `2`, everything else `1`, matching `anth-wire`). Both maps are exhaustive,
    so a new variant without a mapping does not compile. `Response::to_envelope`
    writes `{"status":..,"body":..}` and **omits** `body` for a null value,
    because the host serves `204` only when the key is absent and serves a
    present `null` as the literal text `null`.
  - `transport.rs` — one type per pipe, each modelling only its own fields, and
    **both entry types hold their payload as raw text**. `HttpRequest` carries
    `body` as a `String` (the host never parses an anchor body) and `path()`
    normalizes to exactly one leading slash, because axum strips it from a
    `{*path}` capture. `CliRequest` carries the command **line** as a `String`
    and the ABI payload is that line encoded as a JSON string; a plugin reads it
    through accessors — `line()`, `words()`, `command()`, `positional()`,
    `flag()`, `option()`, `value()`, `action()` — instead of receiving a shape it
    did not ask for. The line is split on whitespace with `'...'`, `"..."` and
    backslash escapes resolved, because the host joins an `argv` into this line:
    a plain split would tear `--name "Ana Sol"` in two, so the quotes are the
    only thing preserving a word boundary. Flag forms are `-v`, `--verbose`,
    `--verbose=true`, `--name value`, `-n value`; `--` ends the flags, `-` alone
    is positional, `-abc` stays one flag. `into_request(commands)` resolves the
    subcommand against the plugin's own alias table, so the routing and the "try
    one of" message come from one source. An object payload is a `400` naming the
    expected shape rather than a command named `{"argv":...}`.
  - `plugin.rs` — the `Plugin` trait: `actions()` plus three mappers
    (`from_http`, `from_cli`, `handle`). The mappers have **no default**: a
    default would let a plugin skip a transport and have the library choose for
    it, which is exactly the reachability defect this contract removes. The
    library owns `http`/`cli`/`direct`, which map then handle, so no plugin
    rewrites the same `match` on a `Result`.
  - `exec.rs` — the `Executor` trait (the capability seam; a test implements it
    over canned answers), plus `exec_bare` which sends `{}` because the host
    decodes arguments into a map and refuses `null`.

  The rule the crate exists to enforce: **a transport is read only in its own
  mapper**. A single envelope with a `kind` tag invites one `match` that
  fabricates the other transport to reuse a handler, and a fabricated request
  silently caps one pipe at whatever shape it invented. With one type per pipe,
  `handle` sees nothing but `Request`, and `plugin call` reaches the `direct`
  pipe with no mapper at all — the caller already speaks the plugin's vocabulary,
  which is also what makes a plugin testable without either transport.

