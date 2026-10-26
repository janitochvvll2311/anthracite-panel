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
  anth-panel/               # the binary: daemon, HTTP API and CLI client
    src/main.rs             #   CLI parsing, exit-code policy, serve_all
    src/dispatch.rs         #   the single request match
    src/lifecycle.rs        #   restart/reboot behind one seam; reboot is opt-in
    src/auth/               #   account, sessions, guard (Authed), store
    src/config.rs           #   flag -> env -> file -> default; Paths, [auth]
    src/api.rs              #   axum router over dispatch; ApiError -> status
    src/watch.rs            #   the CLI's watch loop; interval parser re-export
    src/files.rs            #   one slice of a file inside the readable root
    src/terminal/           #   the one retained state: sessions over a pty
      mod.rs                #     registry: owner, cap, idle expiry, allowlist
      pty.rs                #     the only kernel terminal code: openpty + spawn
    src/socket.rs           #   Unix listener, connection loop, client
    src/table.rs            #   the one bordered-grid renderer
    src/render.rs           #   rows for host-info and plugin list, into Table
    src/log.rs              #   Level, Format, Settings, Logger, Sink, Queue, Nowhere
    src/plugins/            #   manifest, registry, install, runtime/ (wasmi, grants)
    src/repl/               #   the stdin REPL: prompt, editor, keys, history

frontend/                   # Bun workspace, bun.lock committed
  anth-wire/                # @anth/wire: what crosses the boundary, no panel code
    src/error.ts            #   ErrorKind + the one status map, mirrors anth-wire
    src/message.ts          #   payload interfaces, no socket envelope
    src/exchange.ts         #   one JSON exchange over fetch; WireError
    src/stream.ts           #   one SSE subscription over EventSource; StreamStatus
                            #     reads readyState: CLOSED is a refusal, not a blip
  anth-webui/               # @anth/webui: the shell, Vite build, dist/ gitignored
    src/wire.ts             #   presentation only: formatKib, formatUptime
    src/host-info.ts        #   the card: owns its SSE subscription and toolbar
    harness.html            #   dev-only page: one widget, and the browser prompt
  build-shell.sh            # fills backend/anth-panel/www from the Vite build
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

## Toolchain location

`cargo`, `rustc` and `bun` are installed under `/tools` rather than `~`, so they
survive a sandbox rebuild. `/tools/env.sh` exports `CARGO_HOME`, `RUSTUP_HOME`,
`BUN_INSTALL` and the `PATH` entries, and both `~/.bashrc` and `~/.profile`
source it, so a login shell already has them. A non-login shell (for example a
command run through a bare `sh -c`) does not, and needs
`source /tools/env.sh` first.

Three targets are installed: `x86_64-unknown-linux-gnu` (host),
`x86_64-unknown-linux-musl` (what `backend/.cargo/config.toml` pins) and
`wasm32-unknown-unknown` (what a plugin guest is compiled to). `rustfmt` and
`clippy` are present too; the minimal rustup profile omits them, so a rebuilt
toolchain needs `rustup component add rustfmt clippy`.

`gh` lives at `/usr/bin/gh` (system package, not `/tools`) and keeps its
credential in `~/.config/gh/hosts.yml`, which does not survive a sandbox
rebuild. Re-authenticate with `gh auth login --with-token` after one.

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
./build-shell.sh       # builds, then stages the shell into the daemon's www/
```

The Rust binary serves `www/`, so `build-shell.sh` is what makes `cargo run` show
a page. In a release build the same script has to run before `cargo build
--release`, because `rust-embed` reads `www/` at compile time.

`bun run build` alone is not enough to see a frontend change in a browser that
talks to the daemon: it writes `anth-webui/dist`, and the daemon serves
`backend/anth-panel/www`. The two directories are separate on purpose, and a
stale `www/` looks exactly like a stale backend — a widget whose new route
answers `404` and whose new code never runs. When a UI change does not appear,
check which of the two the page actually loaded before touching the Rust side.

A **debug** build reads `www/` from disk per request, so staging the shell needs
no Rust rebuild; the process still has to serve the route it was started with.
A daemon started before a route was added answers `404` for that route forever,
and no frontend change can fix it: restart it. `curl` the route first — a `404`
on `/api/host-info/watch` is a stale process, not a broken widget.

From `plugins/hello/`, which CI also runs:

```bash
(cd backend && cargo test)          # needs no wasm target
(cd frontend && bun test)
./package.sh                        # wasm + bun bundle + manifest digest
```

### Keep test scratch files inside the project

Run every suite with `TMPDIR` pointing at `.tmp/` in the repo, not at the
system temp directory:

```bash
export TMPDIR="$PWD/.tmp" && mkdir -p "$TMPDIR"   # from the repo root
```

Tests create their fixtures under `std::env::temp_dir()`, which honours
`TMPDIR`. Isolating them keeps a fixture bug that deletes "the temp directory"
from reaching `/tmp`, where other processes keep running state: the daemon's
socket, VS Code IPC sockets, the Python `_MEI*` extraction directory. `.tmp/`
is gitignored and safe to delete at any time.

### A test may not read the environment it runs in

A suite that asserts on `io::stdin().is_terminal()` or on `/proc` contents
passes on one machine and fails on the next, and the failure says nothing about
the code. Where production branches on the environment, take the decision as a
parameter and expose a thin caller that reads it:

```rust
pub fn enable() -> io::Result<Self> { Self::enable_if(io::stdin().is_terminal()) }
fn enable_if(interactive: bool) -> io::Result<Self> { .. }   // what the tests call
```

Do not "fix" such a test by asserting a tautology (`assert!(x || !x)`) or by
making the test itself take the branch: that is the vacuous test rule 4 forbids.

### A failure is either the code or the test, never the docstring

When a test and the doc comment above it disagree, one of them is wrong and the
docstring is the specification. Read the prose first: it records what the
behaviour is for. `kill_word`'s comment said one press after `plugin call ` gives
`plugin `, the test expected `plugin call `, and the code matched the test — the
code was wrong. Rewriting the assertion to match the code would have preserved
the bug and made the suite agree with it.

The same rule resolved `word()`: its docstring said "the word under the cursor",
the code returned only the part before the cursor, so Tab could not complete a
word the cursor sat inside. The fix added `prefix()` for the pre-cursor slice and
made `word()` return the whole token, which is what the name promised.

`.github/workflows/v2.yml` runs all of the above in a `rust`, a `web` and a
`plugin` job, so a local pass means a CI pass. Clippy runs with `-D warnings`: a
redundant closure is a build failure, not a suggestion.

Prerequisites, once per machine:

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools     # musl-gcc, for the C dependencies
```

### Git credentials come from `gh`, never from the repo

`git fetch` and `git push` get their credentials from `gh auth git-credential`,
set up once with `gh auth setup-git`. That helper asks `gh` for the token, so no
token lives in `.git/config`, in the remote URL or in any file in the tree.

Do not add a `credential.helper` to `.git/config` that points at a script in
`$HOME`. This repository had one — `!$HOME/.local/bin/github-credential-helper`
— and the script did not exist, so every `git fetch` fell back to an
interactive prompt that nobody could see and hung until it timed out. A helper
that is not there fails worse than no helper at all: the failure looks like a
network problem.

## Manual testing

When asked to "run it so I can try it", start both halves in the background and
leave them up until asked to stop. Do not stop them on your own, and do not
stop them to "clean up" at the end of a turn: the point is that they outlive
the turn.

Everything lives under `.tmp/run/`, never in the system temp directory:

```
.tmp/run/daemon.pid  daemon.log
.tmp/run/vite.pid    vite.log
.tmp/run/anth.sock
```

**Backend.** Use the musl binary, not the gnu one. `backend/.cargo/config.toml`
pins `x86_64-unknown-linux-musl`, so that is what a plain `cargo build`
produces. `backend/target/debug/anth-panel` can be a stale artifact from an
older ABI and will answer with an ABI mismatch that looks like a plugin bug:

```bash
mkdir -p .tmp/run && rm -f .tmp/run/anth.sock
nohup ./backend/target/x86_64-unknown-linux-musl/debug/anth-panel run \
  --addr 127.0.0.1:8080 --socket .tmp/run/anth.sock --plugins plugins/bundles \
  > .tmp/run/daemon.log 2>&1 &
echo $! > .tmp/run/daemon.pid
```

**Frontend.** Bind to `0.0.0.0`, not the default. Vite's default binds
loopback only and on a machine whose `localhost` resolves to `::1` that leaves
`http://127.0.0.1:5173` refusing connections, which reads as the server being
down. `--strictPort` so a second copy cannot silently take 5174 and leave the
browser pointing at the old one.

Run it from the repo root with `bun run --cwd`, not with `(cd dir && ... &)`. A
subshell `cd` runs in the background process only, so a following `echo $! >
../../.tmp/run/vite.pid` resolves the relative path against the original
directory and fails, leaving the previous PID in the file:

```bash
nohup bun run --cwd frontend/anth-webui dev \
  --host 0.0.0.0 --port 5173 --strictPort \
  > .tmp/run/vite.log 2>&1 &
echo $! > .tmp/run/vite.pid
```

`--cwd` is a flag of `bun run`, so it goes before the script name:
`bun --cwd dir run dev` is parsed as `bun run` with no script and prints the
usage text instead of starting anything. Relative paths inside the script
resolve against the target directory, which is what makes this work from the
repo root. `bun run --cwd "$PWD/frontend/anth-webui" dev` if the shell's
relative prefix ever gets in the way.

**Check before reporting it up.** A launched process is not a working one:

```bash
curl -s http://127.0.0.1:8080/healthz          # {"status":"ok"}
curl -s http://127.0.0.1:5173/api/plugins      # through the Vite proxy
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:5173/
curl -s http://127.0.0.1:5173/ | grep -c anth-repl          # 0
curl -s http://127.0.0.1:5173/harness.html | grep -c anth-repl   # 2
```

Then load `/` in a browser: the header, the Host panel and the `Hello` widget.
The harness, where the prompt lives, is at `/harness.html`.

Two traps worth knowing. A stale daemon can leave `.tmp/live/anth.sock` and a
`pid` file pointing at a dead process; start a fresh instance in `.tmp/run/`
rather than reusing them. And confirm the bind actually took effect with
`ss -ltnp | grep 5173` or `/proc/net/tcp` (`00000000:1435` in state `0A`),
because Vite printing `Network:` proves nothing about which addresses answer.

Stop with the PIDs in `.tmp/run/*.pid`. `kill $(cat .tmp/run/vite.pid)` is
enough: bun forwards the signal to the `vite` child, both exit, and port 5173 is
released. Verify with `ss -ltn | grep 5173` rather than assuming, because a
surviving orphan holds the port and the next start fails on `--strictPort`.

## The shell

`anth-panel` serves the built frontend at `/`. The bytes come from `www/`, which
`frontend/build-shell.sh` fills from `frontend/anth-webui/dist`; `www/` is
gitignored apart from `.gitkeep`, because it holds artifacts and not source.

Which source answers depends on the build:

| Build | Source | Why |
| --- | --- | --- |
| release | embedded with `rust-embed` | the binary runs with no `www/` beside it |
| debug | `www/` on disk, read per request | rebuild the frontend, no Rust rebuild |

Do not enable `rust-embed`'s `debug-embed` feature to "simplify" this: it means
embed *always*, which takes away the one thing a debug build is for. And do not
let it pick the debug path itself — rust-embed resolves a relative `folder`
against the *current working directory*, so a daemon started from anywhere but
`backend/` serves nothing. `www::root` uses `CARGO_MANIFEST_DIR`, and `ANTH_WEB`
overrides it.

`www/` is gitignored, so a release build from a clean checkout used to embed
nothing and serve no UI while every test passed. `anth-panel/build.rs` now refuses
a release build with no `www/index.html`, a release-only test asks the router for
`/` and requires the index to mount `anth-app`, and the CI `rust` job runs
`frontend/build-shell.sh` before `cargo test --release`. Do not relax any of the
three: each covers a different way the same hole reopens.

The shell mounts what a plugin declares. `ui[]` entries whose `slot` is
`dashboard.widget` are imported from `/p/<id>/<module>` and appended to the
shadow root. The frame is built once and only the host card is repainted, so a
Refresh does not re-create a live widget; `frontend/anth-webui/src/app.test.ts`
pins that, and it asserts the node exists before comparing it, because comparing
two `null`s passes against the bug.

Two routing facts, both learned by testing rather than reading:

- `/{*path}` does not match `/` on axum 0.8, so `/` needs its own route. Miss it
  and the front page 404s while every asset beside it loads.
- A catch-all loses to a static route, so `/api/*` and `/p/*` still win. But an
  unknown `/api` path must be answered as a JSON `404` by the catch-all: falling
  back to `index.html` with `200` makes a typo look like a live route until the
  JSON parse fails somewhere else.

`harness.html` is a development page and is excluded three times over: Vite
leaves it out of a production build (`ANTH_HARNESS=1` puts it back, with
`public/`), `build-shell.sh` does not copy it, and `www::EXCLUDED` refuses to
serve it. It is also what mounts `anth-repl`, so the browser prompt is reachable
in development and absent from a release for the same reason. `www::normalise`
also refuses any dotfile, because `www/` is tracked in git and so holds
`.gitkeep`.

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
3. **One dispatch site.** A new request adds an arm to `anth-panel/src/dispatch.rs`
   and nowhere else. v0.1.0 repeated each variant across the handler, the client
   and the daemon matches, and nothing tied them together at compile time. The
   HTTP route in `api.rs` calls that same `dispatch`, so the two transports
   cannot answer differently for the same command.
4. **No vacuous tests.** A test must be able to fail. Fixtures that read a file
   panic when it is missing rather than returning an empty value that makes
   every assertion trivially pass. Prefer a real code path — a real listener, a
   real `/proc` fixture tree — over a mock.

### Authentication is one switch, and it defaults to on

`[auth] enabled` in the settings file is the only thing that decides whether
`/api/` needs a session. It defaults to `true`; turning it off is one line that
the operator writes:

```toml
[auth]
enabled = false
```

Two rules hold the shape of this.

`Paths::auth` carries the decision and `ServeOptions` passes it to
`Auth::load(config, required)`. `ServeOptions` derives no `Default` for exactly
this reason: a derived default would make the zero value of the struct the one
setting that removes the protection, and `ServeOptions::default()` would be a
silently open daemon. A caller writes all three fields out.

Nothing else changes. `Authed` stays in every protected signature, so a route is
still visibly protected in the line that declares it, and the switch only
changes what `Authed` demands: with auth off it accepts a request without a
token and still verifies one that is presented, so a stale cookie cannot make an
open daemon answer as somebody it is not. The account, its signing key and the
session table are untouched, which is why flipping the line back restores the
same operator with the same password.

`/api/auth/state` grew a `required` field for the same reason: the browser gate
reads it before it renders, and with auth off it mounts the shell instead of
asking for a password the login route would refuse. The gate paints nothing
until the answer arrives, so an open panel does not flash a login form.

One route keeps asking. `/api/auth/password` rotates the password of "whoever is
calling", which has no meaning without a session: `Authed::account()` refuses
with a `401` that names the setting, rather than picking the only account on the
host and changing a credential nobody proved they own.

The switch is for a host nobody else can reach. A daemon that starts with it off
says so at `WARN`, and `report_registration` stays silent, because warning about
a missing account would suggest a problem the operator just chose.

## The web UI on a small screen

Two rules the shell learned the hard way, both from measuring the real thing:

- **A shadow root does not inherit the document's `* { box-sizing }` reset.**
  The universal selector stops at the boundary, so `theme.css` does reach a
  custom element's `:host` but not its children. A card declared
  `width: min(28rem, 100vw - 2rem)` with `padding: 2rem` then measures
  358 + 64 + 2 = 424px inside a 390px viewport and scrolls the page sideways.
  A component that pairs a viewport-relative width with its own padding must
  declare `*, *::before, *::after { box-sizing: border-box; }` in its own
  stylesheet.
- **Collapse by width, size by pointer.** `orientation` is the wrong query for
  layout: a phone in landscape is 844px wide and would get the sidebar back,
  while a 768px tablet upright would lose it and hide the navigation behind a
  tap for no gain. The sidebar collapses at `max-width: 48rem` and the drawer
  and FAB refuse to exist below `min-width: 48.0625rem`; a test pins that the
  three numbers agree, because a width where the sidebar is hidden and the
  drawer refuses to open leaves the navigation unreachable. Touch sizing is
  `pointer: coarse`, not a width: a tablet is wide and still touched, and every
  control reaches 44px there. Field text never drops below 16px, or Safari
  zooms on focus.

Strings in a stylesheet are checked by tests, but geometry is not: a rule can
be present and still lay out wrong. The measurement that found the 424px card
was a headless Chromium probe over CDP (viewport override, touch enabled, walk
every shadow root, compare `getBoundingClientRect` against
`document.documentElement.clientWidth`), not an assertion on the CSS text.
`pointer: coarse` only matches when touch emulation is on; without
`Emulation.setTouchEmulationEnabled` a phone is measured with a mouse and the
touch rules look broken when they are not.

## Test layout

**One `tests.rs` per module level.** Tests for `lib.rs`, `parse.rs` and the
other files in a directory live in that directory's single `tests.rs`, in one
inner module per source file (`mod frame { .. }`). Do not hang a `tests.rs`
under a directory created only to hold it: `plugins/manifest/mod.rs` plus
`plugins/manifest/tests.rs` is a file and its test, not a module and its
submodule, and the extra level makes the tree lie about the code shape.

`plugins/runtime/` is a real level — it has `mod.rs`, `guest.rs` and `grant.rs`
— so it keeps its own `tests.rs`. A module that is a single file gets no
directory at all.

A sibling `tests.rs` cannot see private items the way an inline `mod tests`
could. Either move the item to `pub(crate)` when production also reads it
(`parse_query`, `encode_payload`, `hex`), or gate the constructor with
`#[cfg(test)]` when only tests build it (`Registry::empty`,
`with_disabled`). That is deliberate: it makes the test-visible surface of a
module explicit.

`anth-panel` is a binary crate, so `tests.rs` under `src/` reaches its
siblings through `crate::`, not `super::*` — `super` there is the `tests`
module itself. Its submodules therefore name what they use
(`use crate::dispatch::Daemon;`) instead of glob-importing the whole crate,
which would also drag in `main`'s items and invite a test to reach past the
module it is meant to cover.

Two exceptions, both because they test something the inline form cannot:

- `anth-plugin/tests/pipes.rs` links the crate as a dependency, which is the
  only way to check the exports a wasm guest actually sees.
- `frontend/**/src/*.test.ts` sits next to its source; that is Bun's
  convention and the typechecker relies on it.

### Blocking work never runs on a runtime worker

A plugin call is wasm behind a 30-second deadline and a file read is disk
I/O; both go through `tokio::task::spawn_blocking`. A read that waits on a
slow mount would otherwise hold the thread that answers `/healthz`, and the
daemon would look dead while it was only busy.

`file read` refuses anything that is not a regular file. The check uses
`std::fs::metadata` on the *path*, before `File::open`: opening a FIFO with no
writer blocks until one appears, so checking after the open hangs on exactly
the case the check exists to refuse — a test that does it this way hangs too.
A cursor also means nothing on a stream, so `offset` and devices are refused by
the same rule.

## The plugin contract

Two libraries hold the contract a plugin must satisfy, one per half, and a
plugin is expected to contain behaviour only.

- **`backend/anth-plugin`** is what a guest links. It is `no_std` (a wasm guest
  has no `std`) and owns four things:
  - `abi.rs` — `ABI_VERSION = 3` and the `anth_plugin!` macro that generates the
    six exports the host checks by name and signature: one entry per transport
    (`anth_http`, `anth_cli`, `anth_direct`) plus the memory contract
    (`anth_abi_version`, `anth_alloc`, `anth_free`). The two imports it provides
    are `anth_exec` and `anth_log`, gated to `wasm32`; off wasm `HostExecutor`
    refuses every capability by name, so a plugin crate still builds and tests
    where no daemon exists. Version 2 replaced the single `anth_handle`, so an
    `abi = 1` bundle has no `anth_http` to call and is refused rather than
    silently broken. Version 3 added the level argument to `anth_log`, so a
    guest asks for `error`, `warn`, `info`, `debug` or `trace` instead of having
    every line printed; a number the host has no name for becomes `info`, never
    `error`, because otherwise a guest could raise its own priority. The level is
    relative to the daemon's: the guest cannot make itself heard when the
    operator asked for quiet. An `abi = 2` bundle is refused by number for the
    same reason as `abi = 1` — its `anth_log` has the wrong arity.
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


## Logging

`anth-panel/src/log.rs` is hand-rolled — 706 lines, 372 more of tests — and adds
no dependency. That is a decision, not an accident: `tracing` would bring a
subscriber, a filter grammar and a macro layer for one process that writes lines
to one stream, and the REPL has to repaint around its prompt, which a generic
subscriber does not know how to do.

Three settings, and the flag beats the variable beats the default:

```
--log-format human|json|off    ANTH_LOG_FORMAT   (default human)
--log-level  error|warn|info|debug|trace         ANTH_LOG_LEVEL (default info)
--log-out    stdout|stderr     ANTH_LOG_OUT      (default stdout)
```

[`Settings::resolve`] takes the environment as a closure rather than reading it,
so the fallback order and the rejection of a typo are a pure function a test can
call; [`Settings::from_flags`] is the thin caller that reads the real thing. A
bad value is reported and the run continues on the default — an operator who
typed `--log-format jsom` must not spend an hour reading `human` output.

- **`Format::Human`** is `STAMP  LEVEL  target: message`, then one `key=value`
  per field. The stamp is UTC RFC 3339 with milliseconds, computed from
  `SystemTime`: `chrono` does not enter the tree for this.
- **`Format::Json`** is one object per line with `ts`, `level`, `target`, `msg`
  and then the fields. `serde_json` escapes a newline for free, so the injection
  a guest could attempt disappears without a sanitiser. A field is a typed
  [`Field`], not a `String`, because `json` must print `3` and not `"3"`.
- **`Format::Off`** prints nothing, not even the startup line. It exists for a
  daemon under a supervisor that already captures output.

**Escaping is the security property.** A guest can send any bytes through
`anth_log`, including a forged newline; without escaping it writes a second log
line that looks like the daemon's own — a guest printed
`anth-panel: listening on /run/anth.sock` as its own line before
`log::escape` existed. `escape` keeps `\n`, `\r` and `\t` visible and turns
every other control character into `\xNN`. Mutation testing found three dead
mutations here, which is why the tests pin the escaping of each class.

**A `Sink` is the stream**, and `Queue` is the one that matters: in `run -i` the
daemon and the REPL share a process, so a daemon holding a stdout sink would
overwrite the prompt. `interactive()` gives the daemon a `Queue` sink; the REPL
drains it above the prompt between keystrokes, using `render::paint_above`. In
the non-TTY path there is no prompt to protect, so the queue is drained straight
to stdout — and the prompt goes to stderr, which is the one split a caller
piping `run -i` needs to know about.

The guest half is [`ABI_VERSION = 3`]: `anth_log` takes a level, so a plugin says
`debug` and the daemon decides whether that survives its own filter. The mapping
is total and the fallback is `info`, never `error`, because a guest that sends
garbage must not be able to raise its own priority. A level the host does not
know is a number a hand-written guest can send; treating it as `info` keeps the
line visible without letting the guest claim urgency.

## Installing a bundle

`plugin install <bundle.zip>` is the only command that puts new code on disk,
and it is deliberately **not** a daemon request. The socket may run a plugin —
but only one the operator already installed, whose digest the host re-checks on
every load. A request that installed would turn "may talk to the daemon" into
"may run anything as the daemon", which is a far larger grant than anything else
in `Request`. So `plugin install` is local: it needs the same write access to
`ANTH_PLUGINS` that copying a directory there by hand would need.

`plugins/install.rs` is ordered so a refusal leaves nothing behind:

1. open the zip and reject more than `MAX_ENTRIES` entries;
2. plan every member — refuse `..`, absolute paths, `./`, backslashes, symlink
   entries, names outside `plugin.toml` / `plugin.wasm` / `web/`, and names that
   appear twice — before creating a single directory;
3. extract into a hidden `.staging-*` directory, counting bytes as they are
   written rather than trusting the size the archive declares;
4. parse the manifest, compare the digest, then **compile** the module, because a
   digest proves these are the approved bytes and says nothing about whether they
   parse; a bundle that installs and traps on every call is worse than one that
   never installs;
5. rename into `ANTH_PLUGINS/<id>`, moving an existing bundle aside first and
   putting it back if that rename fails.

The `Staging` guard removes the directory on every early return, `?` included.
`--force` is opt-in: installing the same id twice is a `Conflict`, not a silent
overwrite of what the operator approved. The daemon reads its registry at
startup, so an install takes effect on the next start — the CLI says so rather
than implying the running daemon noticed.

Two refusals can answer the same question — `safe_member` rejects `..` and the
member allowlist would reject `../x` anyway. Each rule is pinned where it lives:
a test that only drove the archive passed while `safe_member` accepted `..`,
which made it useless as a guard on that function.

`plugin remove <id>` is local for the same reason. The id is validated inside
`plugin_dir` rather than at the call site, because the result feeds
`remove_dir_all`: `plugin remove ../victim` would otherwise delete a sibling of
the registry. Two further guards keep removal narrow — the target must be a
directory, not a symlink, and it must hold a `plugin.toml`, so a registry root
that also holds something else is not pruned entry by entry. A test that only
planted a plain directory would pass with the id validation deleted, because the
manifest guard refuses it too; the escaping-id test therefore plants a *valid*
plugin outside the root, so it fails on the validation and nothing else.

Removing a bundle does not unload it: a running daemon already has the module in
memory and keeps answering until it restarts. That is the same startup-read
model as `install`, and it is why the command says nothing about the daemon.

### Switching a plugin off is written down

`plugin disable` and `plugin enable` are local like `install`, and they exist
because a load failure and an operator decision are different answers that would
otherwise both arrive as an empty row. The preference lives in one file per
registry, `<root>/.state.toml`, never inside a bundle: a bundle's contents are
approved by digest, so a setting stored there would either have to be excluded
from that approval or would let an archive arrive already disabled. One file
also gives `remove` nothing to clean up and the daemon one place to read.

The file records only the negative. A plugin absent from it is on, so a new
install is enabled and a registry nobody has changed needs no file at all. An id
in the file with no directory on disk is ignored on read and pruned on the next
write, which is what stops an uninstall from leaving a tombstone that a later
reinstall would silently inherit.

`Entry::switched_off` is what keeps the two reasons apart, because both end up
as an `Err` in `Entry::enabled`. The operator's reason names the command that
undoes it; a digest mismatch keeps its own text, since switching a broken bundle
back on would not make it run. `Entry::switch_off` drops the module *and the
anchors*: `anchor_owner` treats two plugins claiming one route as a conflict, so
a route this daemon will refuse to serve must not make another plugin's route
ambiguous. Name, version and UI slots stay, because a bundle that loaded and was
then switched off still declares them truthfully.

Tests reload a registry from the same root rather than inspecting `State`, and
the state module is tested directly for id validation: `set_enabled` already
refuses an escaping id through `plugin_dir`, so a test that only went through it
would pass with the guard inside `State::set` deleted. A mutation battery of
eleven edits confirmed that; the one survivor is the `sync_all` before the
rename, which no test can observe without injecting a disk failure.
