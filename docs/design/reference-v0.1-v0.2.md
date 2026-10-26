# Reference architecture: v0.1.0 and v0.2.0

Status: reference document for the v0.3.0 rewrite. It records what the two
previous trees actually built, what they got right, and what failed. It is not a
porting guide: v0.3.0 is allowed to choose differently, but it must do so
deliberately.

## Sources

| Version | Git ref | Commit | State |
| --- | --- | --- | --- |
| v0.1.0 | `v0.1.0` / `origin/main` | `11b3b9e` | shipped and feature-complete |
| v0.2.0 | `origin/v0.2.0` | `f405305` | rewrite in progress, phases E–F unfinished |
| v0.3.0 | current branch | `e63adfe` + planning docs | greenfield: no source yet |

Useful source documents:

- v0.1.0: `README.md`, `AGENTS.md`, `docs/*.md`, `docs/design/*.md`.
- v0.2.0: `README.md`, `AGENTS.md`, `docs/design/v0.2.0.md`, `v1/` (the frozen
  v0.1.0 tree), and `.github/workflows/v2.yml`.

The two trees solve the same product problem with different structures:

- **v0.1.0** is the completed product. Its weakness is structural: every feature
  was added by extending several parallel matches and snapshot-fitting passes.
- **v0.2.0** is the structural correction. Its weakness is coverage: it proves
  the new seams on a few vertical slices and has not ported the product yet.

v0.3.0 should keep v0.1.0's product scope and v0.2.0's structural guarantees,
without inheriting either tree's unfinished edges.

---

# v0.1.0

## Shape

```
backend/                 Cargo workspace, resolver = "2"
  anth-proto/            wire contract: Request/Response + AnthServer/AnthClient
  anth-panel/            one binary: daemon, HTTP API, CLI, embedded web UI
    src/
      api/               axum routes, one module per command family
      auth/              password, JWT, sessions, cookies, middleware
      cli_dispatch/      subcommand dispatch, client, server, settings, plugins
      files/             virtual paths, listings, chunks, archives, trash
      network/           interfaces, addresses, link state, create/edit/delete
      plugins/           manifest, bundle, signature, wasm runtime, web, jobs
      process/           /proc parsing, signals, spawn, snapshot fitting
      terminal/          PTY session, registry, browser bridge
      volumes/           lsblk/mountinfo parsing, mounts, partitions
      accounts/admin     /etc inventory and guarded shadow-utils calls
      config.rs          TOML settings and policy
      daemon.rs          Unix-socket connection loop, uploads, terminals
      handler.rs         request chain and blocking classification
      api/core.rs        router and static-file fallback
      render.rs          bordered CLI tables
      stream.rs, poll.rs, sse.rs
frontend/                Vite + Lit + TypeScript
plugins/wireguard/       reference WASM plugin
docs/                    user-facing contract
```

`anth-proto` is deliberately independent: an alternative client can implement
the wire without depending on panel internals. `anth-panel` owns every
implementation concern.

## Transport and protocol

- Unix socket, newline-delimited JSON via `tokio_util::codec::LinesCodec`.
- `MAX_LINE_LEN = 1 MiB`; a line above it is an error.
- `Request` and `Response` are `serde`-tagged enums with `type`/`data`.
- 49 request variants and 43 response variants.
- Terminal bytes are base64 inside JSON; terminal output is unsolicited and
  ends with `TerminalExited`.
- HTTP is a second transport over the same feature modules, not the same
  dispatch. Each route builds its own request and calls the feature directly.
- SSE repeats a read on an interval (`host-info`, `processes`, `network`);
  `sse::SseStream` owns event names, ids, retry hints and keep-alive.
- The SPA fallback serves the embedded frontend; an unknown `/api` path is a JSON
  `404`, never `index.html`.

## Runtime and dispatch

- `main.rs` parses the CLI and delegates to `cli_dispatch/`.
- `daemon.rs` owns the live connection: request ordering, uploads, terminal
  sessions and their output streams. Stateless requests go to `handler.rs`.
- `handler.rs` is a chain of `Handler`s. Each answers `None` for a request it
  does not own; `CoreHandler` owns the built-in commands.
- `Handler::blocks` marks a request that reads `/etc`, walks `/proc`, touches a
  filesystem or shells out. The connection loop moves those to
  `spawn_blocking`; missing this stalls every client of the connection.
- `Policy` is built from `Config` and passed with the file service to each
  request.
- HTTP routes live in `api/`; `AppState` carries `Config`, `AuthService`,
  `FileService` and the terminal registry. A global middleware protects `/api`.
- The CLI client builds a `Request`, sends it over the socket and renders the
  response. `file read` and `file upload` own their socket loop for chunked
  bytes; everything else uses `client::request_for` + `stream::follow`.
- `repl.rs` reuses the same dispatcher as a one-shot command.

## Configuration and policy

- One `Config` struct in `config.rs`, TOML with
  `#[serde(default, deny_unknown_fields)]`.
- Sections: `[terminal]`, `[process]`, `[network]`, `[firewall]`, `[volumes]`,
  `[accounts]`, `[plugins]`, `[files]`, `[auth]`, plus `socket_path` and a
  global `sudo`.
- Defaults are safe in release: every mutation is off, allowlists are empty,
  `files.root = "/"` but `files.write = false`, auth bootstraps an `admin`
  account with a random password printed once.
- `Config::load_or_create` writes a missing file, but **refuses** to replace a
  file that exists and will not parse. `Config::load_or_default` is the
  client-side path and never writes.
- Debug builds generate a permissive config: global `sudo`, process control and
  spawn, network control/manage, firewall, volumes, accounts control, plugins
  and file writes. Password changes stay off.
- `Config::save` writes through `write_private` (`0600`), because the signing
  key is enough to forge a token.
- Plugin grants live in `<config_dir>/plugins.d/<id>.toml`, one file per plugin,
  mode `0600`, separate from the bundle and from `config.toml`.
- Path defaults: `/etc/anth/config.toml`, `/var/tmp/anth.sock`, plugin root under
  `/var/lib/anth/plugins`, `files.root = "/"`, `volumes.mount_root = "/mnt"`.

## Authentication

- HTTP only. The Unix socket is unauthenticated and relies on file permissions.
- PBKDF2-HMAC-SHA256, digest format
  `pbkdf2-sha256$<iterations>$<salt>$<digest>`; debug builds use a low iteration
  count.
- In-tree HS256 JWT. Every token carries a `sid` (session) and a `kind`
  (`access` or `refresh`); `alg: none` and non-HS256 are rejected.
- Refresh tokens rotate. Sessions are in memory, keyed by `sid`, with a
  fingerprint of the current refresh `jti`. Presenting anything but the current
  refresh token cancels the session.
- `verify_access` consults the session store, so a password change invalidates
  access tokens immediately rather than at expiry.
- Cookies are `HttpOnly`, `SameSite=Strict`, with optional `Secure`.
- A global middleware protects the router once; public routes are an explicit
  allowlist (`/healthz`, `/api/auth/*`). A new route is protected by default.
- `GET /api/auth/session` reports remaining token lifetimes; the frontend uses a
  `withRetry` fetch that renews once on `401`.

## Plugins

- Bundle: `plugin.toml`, `plugin.wasm`, `plugin.sig`, optional `web/`.
- The manifest declares capabilities as **program + argv template + typed
  placeholders**.
- Installation freezes the normalized recipe into a grant. At load the host
  hashes the module against the **grant**, never against the manifest, so a
  rewritten manifest cannot widen authority.
- The guest never sends an argv. It names a capability; the host validates each
  value with an `ArgType` it owns and renders the argv.
- Ed25519 signature over `plugin.wasm`; `[plugins] public_key` is the only trust
  anchor. `require_signature` defaults to true; disabling it is an explicit
  bypass and every install prints a warning. A present signature is still
  checked.
- `wasmi`, one engine per call, ABI 1, exactly two imports (`anth_exec`,
  `anth_log`), four exports checked by name and signature.
- Bounds: fuel (default 50M), 64 MiB guest memory, 32 capability calls per
  invocation, 30 s deadline, 64 KiB log. Fuel alone is not a bound: an exec loop
  is cheap in instructions and expensive in processes.
- Host jobs (`wireguard.*`) are the strong form: the host owns argv, paths and
  secret handling.
- Install is local by default; browser install was added behind
  `[plugins] install_from_web`, with an explicit confirmation.
- UI anchors and API anchors are declared in the manifest. Plugin web assets
  are served from the bundle; the browser module runs on the panel origin.
- Known risk: plugin JavaScript is not hashed or signed, while it runs with the
  panel's origin, cookies and DOM access.

## Frontend

- Vite + Lit + TypeScript; `www/` is embedded with `rust-embed` in release,
  and read from disk in debug.
- `app/panels.ts` is the registry: theme, element, title, description and a
  client factory per panel. The shell never imports a feature component.
- Themes (`System`, `Storage`, `Network`, `Access`, `Extensions`) own a page
  with a tab strip. `#/` is Home, `#/panel/<id>` names a panel.
- Each feature has a typed client under `lib/<feature>/` with injected
  `fetch`/`EventSource`/WebSocket; components depend on the client interface,
  not on `fetch`.
- Shared helpers: `lib/http.ts` (errors, retry), `lib/stream.ts` (EventSource
  lifecycle), `lib/format.ts`.
- Localization: English source strings in `msg()`, Spanish target through
  `lit-localize`; `xliff/es.xlf` and `src/generated/locales/es.ts` are generated
  and committed. `bun run build` runs `lit-localize build`, `tsc`, `vite build`.
- xterm is loaded dynamically for the terminal; CodeMirror is loaded dynamically
  for the editor; both fall back rather than leaving a blank pane.
- Dev harnesses (`*.test.html`) are linked from a component index and are
  deliberately outside the Vite build entry.
- Responsive rules: `@media (orientation: portrait)` switches to the drawer
  layout; safe areas use `env(safe-area-inset-*)`; controls reach 44px; fields
  stay at 16px; reduced motion is respected.

## Testing and operations

- Integration tests under `backend/anth-panel/tests/<module>.rs`, plus unit
  tests in module-level `tests.rs`.
- Tests use real paths where possible: a real `UnixListener`, a real in-process
  echo server, a real router. Socket tests use a unique temp path (pid + nanos).
- The v0.2.0 design records 919 Rust tests and 573 frontend tests for v0.1.0.
- Frontend tests use Vitest + happy-dom and fake clients in `src/test/`.
- No CI in v0.1.0. `cargo fmt`, `clippy` and the suites were local habits.
- Docker builds the frontend with Bun, then the static musl binary, and runs as
  non-root `anth`; nftables needs `NET_ADMIN`; account management needs root.

## What v0.1.0 got right

- Complete product coverage, including the difficult mutations: partitions,
  accounts, archives, trash, terminal and plugins.
- Safe release defaults and explicit per-feature policy switches.
- Guards validated against a fresh, **unfitted** snapshot before every mutation.
- A real separation between the wire contract (`anth-proto`) and panel code.
- Refusal to overwrite a config that will not parse.
- A large integration suite that exercises real daemons and routers.
- Frontend registry + injected clients made panels testable without a server.
- The plugin authority model: host-owned argv, frozen grants, re-hashing.

## Structural debt in v0.1.0

1. **Newline framing forced five truncation engines.** A snapshot over 1 MiB had
   to be shortened, so `process`, `network`, `firewall`, `volumes` and `accounts`
   each grew a `fit_snapshot` pass with its own priority order, `total`,
   `truncated` and `warning`. Silent data loss became a correctness hazard that
   had to be re-proven per feature.
2. **Six per-feature error enums.** `ProcessSpawnErrorKind`,
   `FirewallErrorKind`, `VolumeErrorKind`, `AccountErrorKind`, `FileErrorKind`
   and `NetworkErrorKind` were near-identical, mapped by hand to HTTP status and
   again to CLI exit code. The mappings could disagree.
3. **Dispatch was repeated.** A new operation needed a request variant, a
   response variant and arms in `handler.rs`, `client.rs` and `daemon.rs`;
   nothing tied them together.
4. **State leaked into the daemon loop.** Uploads and terminals had to live in
   `daemon.rs`, which made the connection loop the place to look for protocol
   behavior.
5. **No CI.** A suite could be green locally and fail on a fresh clone.
6. **Config, credentials and signing key shared one file.** This made ordinary
   policy edits and credential rotation the same write path.
7. **Plugin UI ran on the panel origin.** The web module had more authority than
   the wasm guest and less verification.
8. **Some tests encoded assumptions instead of contracts.** The plugin phase
   shipped a guest that read an object where the host sent a string, a fixture
   that skipped silently when the guest was unbuilt, and a UI tag that no module
   registered.

---

# v0.2.0

## Shape

```
backend/                  Cargo workspace, version 0.2.0
  anth-wire/              framing, messages, one error type, interval parser
  anth-host/              pure /proc and os-release parsing, std only
  anth-plugin/            no_std guest contract: ABI, pipes, executor
  anth-panel/             daemon + CLI + HTTP, one binary
frontend/                 Bun workspace
  anth-wire/              browser-side messages, errors, exchange, SSE
  anth-webui/             shell, host card, maintenance, REPL harness
  anth-plugin/            framework-agnostic plugin contract
plugins/hello/            example plugin: Rust guest + Bun frontend + package.sh
v1/                       frozen v0.1.0 tree, buildable until phase F deletes it
.github/workflows/v2.yml  fmt, clippy, tests, web, plugin package
docs/design/v0.2.0.md     plan of record and lessons
```

`backend/`, `frontend/` and each plugin are independent projects with their own
lockfiles and toolchains. A plugin can build with nothing but its own directory.

## Transport and protocol

- Length-prefixed framing: 4-byte big-endian payload length followed by JSON.
- `MAX_FRAME_LEN = 64 MiB`; exceeding it is `ErrorKind::TooLarge`, never a
  shortened answer.
- `PROTOCOL_VERSION = 1` and a `Hello { protocol_version }` handshake. A
  mismatched peer is refused at the first message.
- `WireError { kind, message }` with one `ErrorKind`: `NotFound`, `Forbidden`,
  `Unauthorized`, `Conflict`, `Invalid`, `Unsupported`, `TooLarge`, `Internal`.
- `ErrorKind::http_status()` and `exit_code()` are the only mappings. A status
  can be mapped back to a kind; an unknown status becomes `Internal`.
- `parse_interval` lives in `anth-wire`, so `--watch 500ms` and
  `?interval=500ms` accept exactly the same strings.
- Requests are still `type`/`data` tagged JSON, but one `Request`/`Response`
  pair covers the whole protocol.

## Dispatch and runtime

- `Daemon::handle` is the single dispatch site. The socket, the HTTP route and a
  direct call all reach it.
- `dispatch.rs` contains one `match` over `Request`. Per-feature modules provide
  the work; there are no parallel client/daemon matches.
- `Daemon` owns authority and state: plugin registry, logger, readable root,
  terminal registry, lifecycle, owner identity. Passing these per request would
  let a new transport forget one.
- Disk I/O, `/proc` work, shell-outs and wasm calls run on `spawn_blocking`.
  A blocking read never holds a Tokio worker that could answer `/healthz`.
- `log.rs` is hand-rolled: `human|json|off`, levels, stdout/stderr, passed as an
  `Arc<Logger>` rather than installed globally. The REPL uses a queue sink so
  logs do not overwrite the prompt. Guest messages are escaped.
- HTTP is a thin adapter over dispatch. A route names a command; its body
  carries the arguments at the top level, not the wire envelope.
- Unknown `/api` paths are JSON `404`; the SPA fallback is only for browser
  routes.
- Authentication uses an `Authed` extractor in each protected signature rather
  than a router-wide middleware. A route without it is visibly unprotected.
- `www/` is embedded in release and read from disk in debug. The debug root is
  `CARGO_MANIFEST_DIR`, never the process CWD. A release build without
  `www/index.html` fails at compile time; a release test asks the router for
  `/` and requires the shell.

## Stateful operations

- **Files:** one request reads one slice. `offset` is the identity; the daemon
  keeps no handle. `wait_ms` adds a bounded wait for a file that grows, and
  `waited` distinguishes "end of file" from "nothing arrived in time". The wait
  is capped by the daemon and runs on a blocking worker.
- **Terminals:** the one protocol state that cannot be a value. `TerminalOpen`
  returns a server-generated id; `TerminalRead/Write/Resize/Close` use it.
  Output carries `seq`, which advances only on non-empty bytes. The registry
  owns ownership, a cap, idle expiry and an allowlist; the PTY module is the only
  `unsafe` and uses `posix_spawn`, not `fork`. Terminal support is off unless
  `[terminal] enabled = true`.
- **Lifecycle:** restart and reboot share one `Actions` seam but differ in blast
  radius, privilege and whether settings are re-read. Restart uses `execv` and
  re-reads config; reboot is refused unless `--allow-reboot` and is capped at
  300 s.

## Authentication

- One operator account. The first request may register; every later one is
  refused.
- Argon2id (19 MiB, 2 passes) for the password; HS256 JWT for access; opaque
  refresh tokens stored as SHA-256 digests.
- Access token: 15 minutes. Refresh: 30 days, single-use, rotated on every
  refresh. Reusing a consumed refresh token drops its whole chain.
- Sessions live in `config.toml` under `[auth]`, edited through `toml_edit` so
  operator comments and key order survive. The file is created `0600`; a
  too-readable file is reported/tightened.
- Cookies: `anth_access` on `/`, `anth_refresh` on `/api/auth`; both `HttpOnly`
  and `SameSite=Strict`. Bearer headers are accepted for scripts.
- CSRF: mutating routes compare `Origin` against `Host`, because `SameSite`
  does not separate ports on the same host.
- Auth defaults to **on**. `ServeOptions` deliberately has no `Default`: a
  derived zero value must not be the one that disables protection.
- Public routes: `/healthz`, `/api/auth/*`, the shell and its assets. Everything
  else under `/api/` is protected.

## Configuration

- `config.toml` is thin in v0.2.0: `socket`, `plugins`, `readable`,
  `[terminal] enabled`, `[auth] enabled`.
- Resolution order: flag, environment (`ANTH_SOCKET`, `ANTH_PLUGINS`,
  `ANTH_READABLE`, `ANTH_CONFIG`), file, then a relative default in the working
  directory.
- A missing file is the default state; a file that exists and does not parse is
  refused.
- `auth.enabled` defaults to true; every other switch defaults to off.
- Policy is split across `Paths` (paths and booleans) and `ServeOptions`
  (terminal, reboot, auth). There is no full per-feature policy schema yet.

## Plugins

- Bundle: `plugin.toml`, `plugin.wasm`, optional `web/`; no signature file yet.
- Install is local, not a socket request: writing the plugin root is the same
  access as copying a bundle by hand. The archive is planned first, extracted
  into a hidden staging directory with `O_NOFOLLOW`, validated, compiled, then
  renamed into place. A refusal leaves nothing behind.
- The module digest is re-checked on every load. A rewritten bundle becomes a
  disabled entry with a reason, not a silent skip.
- `plugin.state.toml` records only disabled plugins; absence means enabled.
- Manifest validation: plugin id and alias, ABI, pinned `plugin.wasm` path,
  SHA-256, anchors, capabilities, program allowlist (`hostname`, `uname`, `id`)
  and typed arguments (`string`, `integer`, `switch`).
- ABI 3 has three pipes, one per transport: `anth_http`, `anth_cli`,
  `anth_direct`. There is no shared envelope and no `kind` tag, because a single
  envelope invites a mapper to fabricate the other transport.
- `backend/anth-plugin` is `no_std`; `anth_plugin!` generates the exports. The
  guest gets exactly two imports: `anth_exec` and `anth_log`.
- Runtime bounds: 10M fuel, 64 MiB memory, 30 s deadline, 64 KiB log, and a
  64 KiB stdout ceiling for a host capability. One compiled module, a fresh
  store/instance per call.
- Browser contract (`@anth/plugin`) is plain DOM, not Lit: `PluginWidget`,
  optional `mountWidget`, `PluginEndpoints`, `registerWidget`. This keeps a
  plugin independent of the panel's framework version.
- UI modules are served only when declared in `ui[]`; path normalisation rejects
  `..`, absolute paths and dotfiles. Anchors and modules use separate
  namespaces (`/api/plugins/{id}/...` vs `/p/{id}/...`).

## Frontend

- Bun workspace: `@anth/wire`, `@anth/webui`, `@anth/plugin`.
- `@anth/wire`: message interfaces, `WireError`, one fetch exchange, one SSE
  subscription that reads `readyState` to distinguish a retry from a refusal.
- `@anth/webui`: plain DOM custom elements, no Lit. Flat `DESTINATIONS` drive
  the sidebar, drawer and tab strip. `main.ts` is the composition root and
  injects clients as static properties; components depend on interfaces.
- Auth gate mounts `anth-app` only after a session exists, so the first request
  is authorised and an open panel does not flash a login form.
- Host info owns its own SSE subscription and repaints only its body; the shell
  frame is built once.
- Plugin widgets mount through `WidgetHost`/`slotsFor`; a failed widget reports
  on its own card and the rest continue.
- `build-shell.sh` stages `anth-webui/dist` into `backend/anth-panel/www`,
  keeps `.gitkeep`, excludes `harness.html`, and fails if `index.html` is
  missing or the harness leaked.
- No themes, no i18n, no terminal client yet. The shell is intentionally smaller
  than v0.1.0.

## Testing and CI

- Unit tests live beside each module in a `tests.rs` (one per module level).
- Integration tests live in `backend/anth-panel/tests/`; `anth-plugin` has a
  `tests/pipes.rs` because linking as a dependency is the only way to see the
  guest exports.
- Test scratch files belong under `.tmp/` via `TMPDIR`, never the system temp
  directory.
- A test may not read the environment it runs in; decisions are parameters with
  a thin caller.
- A missing fixture panics with an actionable message; it never returns early
  and leaves assertions vacuous.
- Real entry points are preferred over hand-built envelopes: router, socket,
  serializer, real listener, real `/proc` fixture.
- CI (`v2.yml`):
  - rust: musl target + musl-tools, stage the shell, `cargo fmt --check`,
    `clippy --workspace --all-targets -- -D warnings`, tests debug and release;
  - web: `bun install --frozen-lockfile`, `bun run typecheck`, `bun test`,
    `build-shell.sh`, assert no harness and an index, upload `www`;
  - plugin: backend tests, frontend install/tests, `package.sh`.

## Status and gaps

v0.2.0 is a proven skeleton, not a product:

- Ported: `host-info` (CLI, socket, HTTP, SSE, shell card), `file read` with a
  cursor and bounded follow, terminal wire/registry/PTY (no client), plugin
  list/call/install/remove/enable/disable, auth, restart/reboot, logging.
- Not ported: process, network, firewall, volumes/partitions, accounts, file
  management/archives/trash/editor, full terminal client, full plugin web UI,
  settings UI.
- Deferred: pagination shape, socket authentication, per-module plugin hashes,
  plugin CLI reachability, a full policy schema.
- The v0.2.0 design records 429 Rust and 113 Bun tests at the time of the
  rewrite, which is expected for a partial port.

## What v0.2.0 got right

- The four structural corrections: length framing, one error type, one dispatch
  site, tests that can fail.
- A real CI that tests both debug and release, plus the plugin bundle.
- Pure host parsing in `anth-host`; no `unsafe` outside the PTY.
- Authority owned by `Daemon`, not passed per request.
- A no_std plugin contract with one pipe per transport and no argv.
- Local plugin install with staging, digest re-check and explainable disabled
  entries.
- Client-owned watches for the CLI, SSE only where the browser needs a push.
- The `www` debug/release split with a compile-time guard.
- A small, readable shell instead of a premature feature matrix.

## Structural debt in v0.2.0

1. **Incomplete coverage.** The product features from v0.1.0 are not ported, and
   the rewrite has already accumulated its own deferred list.
2. **Config holds secrets and sessions.** The signing key, password hash and
   live refresh tokens share `config.toml` with operator policy. Every session
   write rewrites the operator's settings file.
3. **No plugin signatures.** The wasm digest is re-checked, but there is no
   publisher signature or trust anchor; plugin web modules are not hashed at all.
4. **Plugin web modules run on the panel origin.** They have the panel's cookies
   and DOM, more authority than the wasm guest.
5. **No socket authentication.** The Unix socket still relies on filesystem
   permissions; there is no `SO_PEERCRED` check.
6. **No pagination.** The frame limit is large, but a list that exceeds it has no
   contract yet.
7. **Thin config.** Only `auth` and `terminal` exist; every ported feature will
   need a policy schema, and that schema does not exist yet.
8. **Plain DOM shell loses the v0.1.0 shell work.** Themes, i18n, the panel
   registry and the rich layout are absent; the v0.2 shell is a demo frame.
9. **Some strings are Spanish in code** (`Cargando…`, `Reintentar`) while the
   rest of the tree is English. This is a localization bug, not a style choice.
10. **The `v1/` tree is duplicated** in the repository to keep it buildable;
    tags and branches already preserve it.

---

# Side-by-side

| Concern | v0.1.0 | v0.2.0 |
| --- | --- | --- |
| Product state | complete | partial vertical slices |
| Crates | `anth-proto`, `anth-panel` | `anth-wire`, `anth-host`, `anth-plugin`, `anth-panel` |
| Socket framing | newline JSON, 1 MiB line | 4-byte length + JSON, 64 MiB frame |
| List overflow | five `fit_snapshot` truncators | no list feature yet; frame refuses |
| Errors | six feature-specific enums, two mappings | one `WireError`/`ErrorKind`, one mapping |
| Dispatch | three hand-written matches per operation | one `Daemon::handle` match |
| HTTP | routes call feature modules directly | routes build `Request` and call dispatch |
| State | daemon-owned uploads + terminals | daemon-owned terminals; file cursor |
| Config | full TOML policy, secrets in it | thin TOML, secrets + sessions in it |
| Auth password | PBKDF2-HMAC-SHA256 | Argon2id |
| Auth tokens | in-tree HS256 + in-memory sessions | `jsonwebtoken` HS256 + file sessions |
| Auth default | bootstrap enables it | `[auth] enabled` defaults true |
| Plugin ABI | ABI 1, two imports, Ed25519 signature | ABI 3, three pipes, no signature |
| Plugin authority | frozen grant, host argv | manifest grant, host argv |
| Plugin install | local + optional web | local only |
| Plugin web assets | served from bundle, not hashed | allowlisted, not hashed |
| Frontend | Vite + Lit, themes, i18n, tests | Bun workspace, plain DOM, no i18n yet |
| Tests | 919 Rust / 573 frontend | 429 Rust / 113 Bun at rewrite time |
| CI | none | rust debug+release, web, plugin |
| Docs | 17 files, user contract | one design doc + README |

---

# Durable lessons for v0.3.0

## 1. Bound the message, not the data

A transport limit is not a data-loss policy. v0.1.0 shortened snapshots to fit a
line; v0.2.0 rejects an oversized frame. v0.3.0 should reject an oversized
message and paginate lists explicitly. No serializer should silently drop rows.

## 2. One error vocabulary

One `ErrorKind` with one HTTP mapping and one exit-code mapping. A feature adds a
kind only when the status genuinely differs. A wire error carries a stable
machine tag and a human message. The CLI and HTTP layers never invent numbers.

## 3. One dispatch site per operation

The socket, HTTP and CLI must reach the same execution path. A new operation may
need a wire type and a transport adapter, but it must not need a second copy of
the business logic. Transport adapters only translate.

## 4. State is a protocol decision

Most reads are values: a snapshot, a file slice at a cursor, a plugin reply. Keep
state only where the operation cannot be represented as a value: a live PTY, an
upload in progress, a session table. State lives in a named service owned by the
daemon, not in the connection loop.

## 5. The client owns repetition; the browser gets a push

A CLI watch is a loop over one request on one connection. Do not add a frame type
or daemon timer for it. A browser cannot hold that loop, so SSE is justified
where a page watches a value; use it only where a second real consumer exists.
Both paths must use `MissedTickBehavior::Delay` and the same interval parser.

## 6. Blocking work never runs on a runtime worker

`/proc`, `/etc`, disk I/O, `spawn_blocking`-worthy shell-outs and wasm calls must
leave the Tokio worker free. A test that pins this uses a single worker on
purpose; a multi-worker test passes even when the code is wrong.

## 7. Policy lives at one authority seam

Build an immutable policy from config once. Pass a typed sub-policy to the
feature, not the whole config and not raw booleans. Every mutation validates
against a fresh, unfitted snapshot; fitting is a display concern. Release
defaults deny; empty allowlists deny; debug defaults are explicit and printed.

## 8. Fail closed on configuration

A missing config bootstraps; a config that exists and will not parse is refused.
Never overwrite the operator's file to "recover". Keep secrets, sessions and
grants out of the policy file. Atomic writes, `0600`, temp + fsync + rename.

## 9. Authentication is on by default

One operator account is enough until roles have a consumer. Access tokens are
short-lived and revocable through a session record; refresh tokens are opaque,
single-use and rotated. Cookies are `HttpOnly`/`SameSite=Strict`; scripts may use
a bearer header. A credential change invalidates sessions. The socket needs its
own peer-credential check, not an implicit trust in filesystem permissions.

## 10. A plugin never owns authority

The guest names a capability; the host owns the program, the argv, the paths and
the secret handling. Freeze the normalized recipe in a grant; re-check the
bundle's digest on every load; sign the bundle; serve only declared web assets;
hash every declared asset. Bound fuel, memory, wall time, capability calls, log
bytes and reply size. A signature proves approval, not safety.

## 11. Plugin UI is the larger attack surface

A wasm guest is sandboxed with a capability allowlist. A web module runs in the
operator's browser, on the panel's origin, with the panel's cookies and DOM.
Signing it is necessary but not sufficient. Prefer an isolated iframe or a
separate origin with a narrow host bridge; do not let arbitrary plugin script
read the panel session.

## 12. Tests must be able to fail

A missing fixture must panic, not return early. A test must drive the real entry
point: the router, the socket, the serializer, the real binary. A fake runner is
fine for a guard matrix, but it cannot catch a wrong argv; keep a real-tool test
for `sfdisk`/`mkfs` where available. Assert the exact bytes where the contract is
bytes. Run both debug and release, because the embedded shell is a different
branch.

## 13. No speculative abstraction

A registry, a handle, a generic operation framework or a stream type needs a
second real consumer before it exists. v0.2.0 explicitly rejected a file-handle
registry because re-opening by path and seeking was enough. That instinct is
correct.

## 14. Keep the shell derivable from data

A panel registry plus injected clients lets the shell, sidebar, drawer and tab
strip stay in sync and lets every panel mount without a server. A plugin contract
must be framework-agnostic; the panel may use Lit, a plugin should not be forced
to. Generated assets, locale bundles and harnesses need explicit build and
exclusion rules.

---

# Reading map

| Question | Read |
| --- | --- |
| What did v0.1.0 ship? | `git show origin/main:README.md`; `git show origin/main:docs/cli.md` |
| How did v0.1.0 transport work? | `git show origin/main:docs/socket-protocol.md`; `.../docs/api-endpoints.md` |
| Why was v0.2.0 started? | `git show origin/v0.2.0:docs/design/v0.2.0.md` |
| What is v0.2.0's contract? | `git show origin/v0.2.0:AGENTS.md`; `.../backend/anth-wire/src/*` |
| How does the plugin boundary work? | `git show origin/v0.2.0:backend/anth-panel/src/plugins/*`; `.../backend/anth-plugin/src/*` |
| How does v0.1.0 guard mutations? | `git show origin/main:docs/design/plugins.md`; feature docs under `docs/` |
| What did v0.2.0 finish? | `git show origin/v0.2.0:docs/design/v0.2.0.md` (phases A–F) |
| What is the v0.3.0 plan? | `docs/design/v0.3.0.md` |
