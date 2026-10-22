# AGENTS.md

Repository-specific knowledge for agents working on Anthracite Panel.

## What this is

Two sibling projects: a Rust backend and a Vite/Lit frontend. The backend ships a
single binary, `anth-panel`, that serves the Unix-socket daemon, the CLI client and
the HTTP panel with an embedded web UI. License is Apache-2.0. User-facing docs
live in `docs/` and are indexed from `README.md`.

## Layout

```
README.md                   # project overview, indexes docs/
.vscode/settings.json       # nests *.test.ts and *.test.html under their *.ts
docs/
  cli.md                    # commands, flags, exit codes
  api-endpoints.md          # HTTP/WebSocket routes, bodies, SSE frames, error shape
  socket-protocol.md        # anth-proto wire format
  terminal.md               # PTY sessions, browser route and its current risks
  processes.md              # process monitoring, signals, limits and controls
  files.md                  # filesystem root, chunked transfers, write policy
  volumes.md                # storage inventory, mount/unmount policy
  design/                   # design notes for the TTY protocol and web terminal
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
      daemon.rs             #   Unix socket listener, sessions, uploads
      handler.rs            #   handler chain: request dispatch, blocking policy
      client.rs             #   CLI client: builds a Request, prints the Response
      api.rs                #   axum HTTP API + WebSocket upgrade + rust-embed SPA
      app.rs                #   AppState shared by the routes
      ws.rs                 #   generic JSON-over-WebSocket wrapper
      hostinfo.rs           #   host inspection from /proc and /etc/os-release
      process.rs            #   process table from /proc and signal policy
      network.rs            #   interfaces from sysfs/proc, addresses and up/down ioctl
      command.rs            #   sudo-capable command runner shared by nft, mount and umount
      firewall.rs           #   managed nftables table and rules
      volumes.rs            #   lsblk/mountinfo parsers, mount and unmount policy
      files.rs              #   virtual paths, listings, chunked reads and atomic uploads
      render.rs             #   bordered table renderer for the CLI
      repl.rs               #   interactive session loop for `-i`
      stream.rs             #   repeat a request: interval parsing + frame output
      poll.rs               #   repeat an async read on a period (shared timing)
      text.rs               #   shared trimmed-file read for the inspectors
      sse.rs                #   SSE framing: named events, ids, keep-alive
      terminal/             #   PTY session, registry, shell detection, token helper, browser bridge
      terminal_client.rs    #   interactive terminal for the CLI
    www/                    #   rust-embed folder; holds only .gitkeep in the repo
    tests/                  #   api.rs, client.rs, config.rs, daemon.rs, files.rs, hostinfo.rs,
                            #   render.rs, stream.rs, sse.rs, terminal.rs, terminal_client.rs,
                            #   terminal_exit.rs, terminal_web.rs, volumes.rs
frontend/                   # Vite + Lit + TypeScript, package name `anth-panel`
  vite.config.ts            # proxies /healthz and /api (including WebSocket)
  vitest.config.ts          # frontend unit tests, happy-dom environment
  lit-localize.json         # i18n config: English source, Spanish target
  xliff/es.xlf              # extracted messages and their Spanish translations
  src/
    app/                    # shell, panel registry, hash router, i18n runtime
    generated/              # lit-localize output; regenerate, never edit
    components/             # feature components, tests and harnesses
    harness/                # anth-harness page shell + shared harness.css for dev pages
    lib/ws/                 # generic JSON WebSocket transport and tests
    lib/terminal/           # terminal interface, protocol, WebSocket adapter and tests
    lib/processes/          # process client, wire types, sort/display helpers and tests
    lib/network/            # network client, wire types, rate/sort helpers and tests
    lib/firewall/           # firewall client, wire types and display helpers with tests
    lib/files/              # file client, wire types, path/display helpers and tests
    lib/hostinfo/           # host-info client, wire types and display helpers with tests
    test/                   # setup and the fake WebSocket, EventSource, process, network,
                            # firewall, file and host-info clients
  public/  index.html
docker/entrypoint.sh        # starts `anth-panel run`
Dockerfile                  # bun frontend stage, musl backend stage, alpine runtime
compose.yaml                # container with NET_ADMIN for nftables
```

`anth-proto` is deliberately separate so alternative or custom tooling can implement
the protocol without depending on the panel. `anth-panel` depends on it.

## Build and test

Run these from `backend/`. The project targets **`x86_64-unknown-linux-musl`
only**: `.cargo/config.toml` pins it, so a bare `cargo build` (no `--target`) is
the supported invocation and produces the same static binary the Dockerfile
ships. Do not add `--target x86_64-unknown-linux-gnu`; a glibc build diverges
from production and needs a toolchain the project does not install.

Prerequisites, once per machine:

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools     # provides musl-gcc for the C dependencies
```

Then:

```bash
cargo build  --workspace
cargo test   --workspace
cargo fmt --check
cargo clippy --workspace --all-targets
```

There is no CI configuration, no `rustfmt.toml` and no `clippy.toml` — defaults apply.

### The backend compiles without a frontend build

`anth-panel/src/api.rs` embeds `www/` with `#[derive(RustEmbed)]`. In development `www/`
holds only a `.gitkeep` and Vite serves the UI, proxying the API routes itself — see
`frontend/vite.config.ts`. To exercise the embedded UI, build the frontend and copy its
output into `www/` first:

```bash
cd frontend && bun install && bun run build && cp -r dist/. ../backend/anth-panel/www/
```

`bun run build` regenerates the locale bundle first, then type checks and
bundles. `bun run i18n:extract` rewrites `xliff/es.xlf` after message changes;
fill in the `<target>` of each new unit before building, or the build fails on
the missing translation.

Generated files under `www/` are gitignored except the placeholder. A missing folder
surfaces as `no associated function named 'get' found for struct 'Asset'`. `rust-embed`
caches the folder per compilation of the file that declares it, so touching
`api.rs` is the reliable way to re-embed after copying new assets.

### Frontend unit tests

Vitest tests live next to the source as `frontend/src/**/*.test.ts`.
`frontend/vitest.config.ts` uses `happy-dom`; the component tests mock xterm so they
do not need a real canvas, and the process tests use an injected fake client rather
than a daemon. Run them from `frontend/`:

```bash
bun run test          # vitest run
bun run test:watch
```

`bun` is the only package manager: `npm`, `npx` and `corepack` are not installed.

## Running locally

Development uses two processes; the frontend proxies to the backend:

```bash
cd backend  && cargo run -p anth-panel -- run --addr 127.0.0.1:8080   # daemon + API
cd frontend && bun run dev                                            # UI on :5173
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
  `#[serde(tag = "type", content = "data")]`. The request/response commands are
  `Echo`, `HostInfo`, the process variants (`ProcessList`, `ProcessSignal`), the
  filesystem variants (`FileList`, `FileRead`, `FileMkdir`, `FileRename`,
  `FileDelete` and the chunked `FileWrite*` upload flow), the volume variants
  (`VolumeList`, `VolumeMount`, `VolumeUnmount`) and the
  terminal variants (`TerminalOpen`, `TerminalInput`, `TerminalResize`,
  `TerminalClose`); terminal output arrives unsolicited as
  `Response::TerminalOutput` and ends with `TerminalExited`. Add a new operation by
  extending both enums plus the `match` arms in `anth_panel::daemon` and
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
  character count so multi-byte text stays aligned. `render_host_info`,
  `render_process_snapshot`, `render_network_snapshot`, `render_firewall_snapshot`
  and `render_file_listing` map snapshots onto it. It is a local renderer, not a
  dependency.
- **Authentication**: `anth_panel::auth` is the browser boundary, not the socket.
  `password.rs` is PBKDF2-HMAC-SHA256 with the digest format
  `pbkdf2-sha256$<iterations>$<salt>$<digest>` (base64url, no padding); the
  iteration count comes from `[auth] iterations` and debug builds default low so
  the suite stays fast. `jwt.rs` signs HS256 only and rejects `alg: none` and any
  other algorithm; every token also carries a `sid`, which is what ties it to the
  session store. `session.rs` keys sessions by `sid` and keeps a fingerprint of
  the current refresh `jti`. `verify_access` asks the store whether the session is
  live, so a password change invalidates access tokens immediately instead of at
  their expiry. Presenting anything but the current refresh token for a live
  session cancels that session, which covers a replayed rotation and an access
  token offered as a refresh token alike. Policy fields sit behind an inner lock
  (`apply_config`), so a settings save keeps the sessions; only a credential
  change rebuilds the service. `cookies.rs` owns the
  `HttpOnly`/`SameSite=Strict` attributes and the optional `Secure` flag.
  `middleware.rs::protect` wraps the whole router once, so a new route is
  protected by default and a public one needs an explicit exemption; it accepts
  the `anth_access` cookie or a Bearer token. Every token carries a `kind`, and
  only `access` reaches a route: a refresh token passes the signature check but
  is refused, which is what stops a leaked one from being a 12-hour credential.
  `refresh` does its check, spend and re-issue under one write lock, so two
  callers holding one token get one pair between them. `GET /api/auth/session`
  reports the *remaining* seconds from each token's `exp`, and the shell hands
  every panel client a `withRetry` fetch (`lib/http.ts`) that renews once on a
  `401`, so a tab that outlives its access token recovers on the next call. `routes.rs` serves
  `/api/auth/{session,login,refresh,logout,password}`. The daemon socket itself
  is still unauthenticated — this layer covers HTTP only.
- **Config API**: `config_api.rs` serves `GET`/`PATCH /api/config`. The view is a
  projection of `Config` with secrets replaced by `password_set`/`token_set`
  booleans. A patch replaces only the keys it names, is round-tripped through
  TOML to prove it reloads, then applied in memory with
  `AppState::replace_config` (policy only, sessions untouched) and written with
  `AppState::persist`. Disabling auth from the panel is refused on purpose; that
  is a host-only decision. `AppState::replace_auth` is the other path: it swaps
  in a new service and drops every session, which is what `POST
  /api/auth/password` needs.
- **Settings CLI**: `settings.rs` backs `auth status|enable|set-password` and
  `config get|set`. They edit the file directly and never touch the daemon.
  `set_password` reuses `password::hash_password_with_iterations`; `set_key`
  validates before writing so a bad value leaves the file untouched.
- **Config**: `anth_panel::config::Config` (`socket_path`, `[terminal]`, `[process]`,
  `[network]`, `[firewall]`, `[volumes]` and `[files]`) is TOML with
  `#[serde(default, deny_unknown_fields)]`. Defaults: config at
  `/etc/anth/config.toml`, socket at `/var/tmp/anth.sock`, global `sudo = false`,
  `terminal.shell = "auto"`, empty token, `max_sessions = 4`,
  `process.control = false` with `process.spawn = false` and an empty
  `spawn_allow`, `network.control = false`, `network.manage = false`,
  `firewall.control = false`, `volumes.control = false` with
  `volumes.mount_root = "/mnt"`, legacy `network.sudo`/`firewall.sudo` and
  `firewall.sudo_password` are accepted and ignored,
  `files.root = "/"`, `files.write = false`, `max_entries = 5000`,
  `max_preview_bytes = 1 MiB` and
  `max_upload_bytes = 64 MiB`. `[auth]` defaults to `enabled = true` with an
  empty `password_hash`, `username = "admin"`, a generated `jwt_secret`,
  `access_token_secs = 900`, `refresh_token_secs = 43200` and
  `cookie_secure = false`. `Config::load_or_create` fills the empty password hash
  and signing key on first run, prints the generated password once and stores
  only the digest. `Config::save` writes through `write_private`, which opens with
  mode `0600` and tightens an existing file, because the signing key in it is
  enough to forge a token. `auto` resolves `$SHELL`, then the account's
  login shell from `/etc/passwd`, then `/bin/sh`. The server subcommands load
  it through `Config::load_or_create`, which returns `Result`: it writes a default
  file when the path does not exist, so a first run bootstraps
  `/etc/anth/config.toml`, but it **refuses** when a file exists and will not
  parse. Answering a parse error with defaults used to overwrite the operator's
  settings, which turned a stale plugin grant into a wiped `files.root` and a
  new `jwt_secret`; the file is now left byte for byte and the server exits `1`.
  The client subcommand uses `Config::load_or_default`, which never writes. `Config::server_default` returns
  `Config::dev_default` in debug builds, which sets global `sudo = true`,
  `process.control = true`, `network.control = true`,
  `firewall.control = true` and `files.write = true`; release builds keep the
  controls off.
- **Panel routes**: `GET /healthz`, plus one route per command under `/api`:
  `GET /api/host-info`, `GET /api/host-info/watch?interval=2s` (SSE),
  `POST /api/echo`, `GET /api/terminal` (WebSocket),
  `GET /api/processes`, `GET /api/processes/watch?interval=2s` (SSE),
  `POST /api/processes/{pid}/signal` and `POST /api/processes/spawn`; the network routes are `GET /api/network`,
  `GET /api/network/watch?interval=2s` (SSE) and
  `POST /api/network/{name}/state`, `POST /api/network`,
  `PATCH /api/network/{name}`, `POST /api/network/{name}/rename` and
  `POST /api/network/{name}/delete`; the firewall routes are `GET /api/firewall`,
  `POST /api/firewall/rules`, `POST /api/firewall/rules/{handle}/delete` and
  `POST /api/firewall/flush`; the account routes are
  `GET /api/accounts`, `POST /api/accounts/users`,
  `POST /api/accounts/users/{name}`, `POST /api/accounts/users/{name}/delete`,
  `POST /api/accounts/users/{name}/password`, `POST /api/accounts/groups`,
  `POST /api/accounts/groups/{name}` and
  `POST /api/accounts/groups/{name}/delete`; the filesystem routes are
  `GET /api/files`, `GET /api/files/content`, `GET /api/files/download`,
  `POST /api/files/mkdir`, `POST /api/files/rename`, `POST /api/files/delete` and
  `POST /api/files/upload`. The route names the command, so
  bodies carry the command's arguments at the top level (`{"message":"hi"}`) with no
  `type`/`data` envelope; the envelope stays a `anth-proto` concern for socket clients
  only. See `docs/api-endpoints.md`. Every failure is `{"error": "..."}` via
  `api::ApiError`, including bodies axum rejects before the handler runs. A fallback
  static handler serves the embedded SPA and falls back to `index.html`, except
  under `/api`, where an unknown route is a JSON `404`. Table tests
  exercise the router through `tower::ServiceExt::oneshot`.
- **Dispatch**: `anth_panel::handler` owns the request chain. A `Handler` answers
  `None` for a request it does not know and the next handler tries, which is the
  seam a plugin will use later. `CoreHandler` holds every built-in command, and
  `Handler::blocks` states whether a request may block, so the connection loop no
  longer keeps its own list. `daemon.rs` keeps only what needs a live connection:
  uploads and terminal sessions. A new command therefore touches the protocol,
  the client, the CLI and one handler arm.
- **Web app shell**: `frontend/src/app/` holds the shell (`anth-app`), the panel
  registry, a pure hash router (`#/panel/<id>`) and the `lit-localize` runtime.
  The shell renders one card per registry entry and mounts the chosen custom
  element itself, because a tag name is data rather than markup. Clients are
  created by the registry and can be replaced through `clientFactory`, which is
  how the shell tests mount every panel without a socket. `index.html` mounts the
  shell; only `index.html` is a Vite build entry, so harnesses stay out of `dist/`.
- **Localization**: source strings are English and wrapped in `msg()`. `bun run
  i18n:extract` writes `frontend/xliff/es.xlf`, `bun run build` regenerates
  `src/generated/locales/es.ts` before type checking. The locale is stored in
  `localStorage` under `anth.locale`. Generated locale files are committed.
- **Process manager**: `process.rs` reads `/proc`, resolves users from `/etc/passwd`
  and enforces the signal allowlist and ownership policy through one
  `ProcessPolicy` built from `[process]` per request. `[process] control` and
  `[process] spawn` both default to `false`; the daemon refuses pid 1, its own pid,
  other users' processes and anything outside `term/kill/hup/int/quit/stop/cont`.
  Spawning additionally requires the canonicalised program to appear in
  `spawn_allow`, and an empty list denies every request even with `spawn = true`.
  The child is detached with `Stdio::null()` and reaped by a thread per child, so
  a short-lived spawn does not leave a zombie and the global `SIGCHLD`
  disposition (which the PTYs depend on) is untouched. Snapshots cap rows at 2000,
  truncate commands to 256 characters and then fit the serialized snapshot to the
  1 MiB line cap by shortening commands further and dropping the least busy rows.
  Command shortening sets `truncated`; dropping rows also adds a human-readable
  warning. The `/proc` walk runs on a blocking worker so it cannot stall the
  async connection loop.
  CPU percentages are computed by the client from `cpu_ticks`, `clock_ticks` and
  `timestamp_ms`; the daemon never keeps a sampler.
- **Frontend process manager**: `anth-process-table` is a controller that owns
  sort state and renders one of two renderers: `anth-process-flat-table` (compact
  table) or `anth-process-tree` (free-form tree with CSS guides and SVG
  expand/collapse chevrons). Both show PID, COMMAND, THREADS, USER, STATE, MEM
  and CPU; PPID is used only for nesting. Flat headers are all sortable; the tree
  omits PID and COMMAND sorting. A tree command stays on one line while the
  selected-process preview shows the full command, left-aligned and vertically
  centered in the footer. `anth-process-manager` owns filtering, the flat/tree
  toggle, SSE lifecycle, confirmed signals and the spawn modal, and only depends
  on the injected `ProcessClient`. It ignores stream snapshots while a signal is
  in flight and refreshes once it finishes. Spawn controls come from the
  snapshot's `spawn_enabled`/`spawn_allow`, so a disabled daemon shows a
  disabled button with the reason instead of a form that cannot succeed. Both are
  exercised by Vitest component tests and the
  `src/components/anth-process-manager.test.html` dev harness.
- **Filesystem**: `files.rs` validates `[files] root`, normalizes virtual
  paths, rejects `..` and symlink escapes, lists directories, reads stateless chunks
  and performs atomic uploads through a per-connection temporary file. Writes are
  gated by `[files] write`, which defaults to `false`. `FileError` carries a
  machine-readable kind so the HTTP layer can map `404`/`403`/`409`/`413`.
  `render_file_listing` is the CLI table; the API streams uploads and downloads
  through the chunked flow instead of buffering a whole file, and inline previews
  force `text/plain` plus `nosniff` except for a safe raster-image allowlist.
- **Frontend file manager**: `anth-file-manager` owns breadcrumbs, navigation,
  sorting, hidden-file filtering, previews, uploads, archives, the recycle bin,
  the text editor and confirmed writes. It only depends on the injected
  `FileClient`; `lib/files/file-client.ts` implements `/api/files` and
  `lib/files/display.ts` owns path, size and sort helpers. The read-only state
  comes from the listing's `write_enabled`.
  `anth-file-editor` is the CodeMirror editor behind the preview's Edit button:
  it imports CodeMirror dynamically, keeps the buffer dirty when a save is
  refused, and sends the mtime it loaded as the save precondition. Tests live
  under `src/components/` and `src/lib/files/`; the
  `src/components/anth-file-manager.test.html` file is the dev harness.
- **Frontend editor**: `lib/editor/language.ts` maps a file name to a grammar
  (pure, tested) and `lib/editor/codemirror-setup.ts` owns everything
  CodeMirror-specific behind the four-method `EditorHandle` the component sees.
  A failed dynamic import falls back to a textarea so the editor is never a
  blank pane.
- **Network manager**: `network/mod.rs` reads `/sys/class/net` and `/proc/net/dev`,
  resolves IPv4/IPv6 with `getifaddrs(3)`, derives each interface's `kind` and
  `physical` flag from sysfs, and enforces `[network] control` plus the
  loopback-down guard before calling `SIOCSIFFLAGS`. `network/manage.rs` owns
  create/edit/rename/delete behind the separate `[network] manage` switch:
  creation is limited to virtual kinds, rename requires the link down, and
  delete refuses a loopback, the interface the request's `peer` resolves to
  (`interface_for_peer`) and the last default-route carrier
  (`default_route_interfaces`). `NetworkDelete` carries `peer` because the
  socket cannot see the client; the HTTP route fills it from `ConnectInfo` via
  the `PeerAddr` extractor, which degrades to `None` instead of a `500` when the
  router has no connect-info layer. Global `sudo` defaults
  to `false` in release and `true` in the generated debug config; it tries
  `sudo -n ip link set` first and falls back to the ioctl when sudo cannot
  elevate. The guard reads the flags with `SIOCGIFFLAGS` before either path, so
  an unreadable sysfs `flags` file cannot fail open. Snapshots cap the list at
  2000 interfaces and report `total` and
  `warning` when shortened; the fit loop also enforces the 1 MiB serialized
  line cap for address-heavy rows. Up/down runs on a blocking worker.
  `anth-network-manager` follows the SSE stream through an injected
  `NetworkClient`, computes RX/TX rates from cumulative counters and only shows
  controls when the snapshot says `control_enabled`. It ignores stream snapshots
  while an action is in flight and refreshes once it finishes. When
  `manage_enabled` is true the toolbar offers a create form and the detail panel
  offers MTU edit, rename and delete. `manageBlockReason`/`renameBlockReason`
  mirror the daemon guards (loopback, link-down rename) so a row is disabled
  with the reason in its `title` instead of offering an action that must fail;
  they are a hint, never the policy, and cannot evaluate the peer-address guard.
  Deleting a physical device is allowed but the confirmation says the host will
  lose the link. The dev harness is
  `src/components/anth-network-manager.test.html`; tests live beside the
  component and `lib/network/`.
- **Firewall**: `firewall.rs` shells out to `nft` with an argument vector and owns a
  dedicated `inet anthracite input` table. `deny` initializes the table and chain
  and adds a tagged drop rule; `allow` removes the matching managed drop, and
  `delete`/`flush` only touch that chain.
  `[firewall] control` defaults to `false`; status remains read-only. Global
  `sudo` defaults to `false` in release and `true` in the generated debug
  config, which runs `nft` through `sudo -n` and falls back to direct nft when
  sudo cannot elevate. The daemon otherwise relies on `CAP_NET_ADMIN` or root.
  The Docker runtime installs `nftables` and sets `CAP_NET_ADMIN` on `nft`; the
  container needs `--cap-add NET_ADMIN`. Blocking nft calls run on a blocking
  worker. `parse_rules` keeps only rules from the managed `input` chain, so the
  snapshot can never advertise a rule that `delete`/`flush` would fail to reach.
  `fit_snapshot` mirrors `process.rs`: it drops `ruleset` first and then the
  rules with the highest handles until `Response::FirewallSnapshot` fits
  `MAX_LINE_LEN`, setting `total`, `truncated` and `warning`. Mutations read
  through `read_table`, which is unfitted, so shortening never hides a rule from
  `allow` or `flush`.
- **Plugins**: `plugins/` is the authority boundary. The model is a kitchen: the
  daemon runs programs (chef), the manifest declares which programs and with what
  shape (recipe), and the plugin only supplies declarative values (helper). A
  plugin **never** sends an argv — `manifest::render_argv` builds it from a
  template frozen in the `[plugins] grants` at install, validating each value
  with an `ArgType` the host owns. Programs must be on `PROGRAM_ALLOWLIST`, or
  the capability names a `job` the host implements its own argv for. `bundle.rs`
  extracts to a staging directory, refuses any member that is not
  `plugin.toml`, `plugin.wasm` or under `web/` (reusing
  `files::archive::safe_member` plus `O_NOFOLLOW`), hashes the module and only
  then renames. `load` re-hashes on every start and compares the version, so a
  rewritten bundle is disabled rather than trusted; the module path is pinned to
  `plugin.wasm` so a rewritten manifest cannot re-point it. `plugin
  list|install|remove|enable|approve` run locally like `auth` and `config`, and
  `approve` requires `--accept-capabilities` because widening a grant is never
  implicit. `GET /api/plugins` is read-only and reads the bundle directory, so
  the daemon stays unaware of which plugins the panel shows.
  `plugins/runtime.rs` is the `wasmi` host: one engine per call with
  `consume_fuel` and `StoreLimits`, exactly two imports (`anth_exec`,
  `anth_log`), and the four documented exports checked by name and signature
  before a request is written. `anth_exec` reads the frozen recipe from the
  **grant** and renders argv there; the manifest is never consulted for
  authority. Fuel bounds guest instructions only, so the host adds three more
  bounds: 32 capability calls per invocation, a 30 s deadline, and 64 KiB of
  log. `install` and `load` both call `runtime::probe`, because a digest
  proves the bytes were approved, not that they parse. `plugin call` honours
  `[plugins] enabled` like every other path. `plugin call <ID>
  [REQUEST]` is the CLI anchor; tests drive a real `plugins/guest.wat`. Design
  and threat table: `docs/design/plugins.md`.
- **Accounts**: `accounts.rs` reads `/etc/passwd` and `/etc/group` for what the
  panel can change, and `getent` for what the host actually resolves, tagging
  every entry `local` or `nss`; an `nss` entry is read-only. Lock state comes
  from one bit per line in `/etc/shadow`, and an unreadable shadow file yields
  `null`, shown as `?`, rather than `false`. `accounts_admin.rs` never edits
  those files: `plan_*` validates a request into a `PlannedCall` (program, argv,
  optional stdin) and the daemon runs it. The split is what the tests assert —
  the exact argv vector, and that no password ever reaches argv because
  `chpasswd` takes it on stdin. `PlannedCall`'s `Debug` redacts the stdin field,
  which is the only reason it is safe to log. Guards live in `Guard`: uid/gid 0,
  the daemon's own account, the last member of `sudo`/`wheel`/`admin` (counting
  a user whose primary group it is, not only `/etc/group`'s member field),
  `nss` entries, and system accounts or privileged groups without their opt-in.
  Root is deliberately not an escape hatch for the administrator guard. Snapshots
  are read unfitted for validation so a truncated list never looks like a
  missing account. `collect_detailed` returns the snapshot and the
  `login.defs` ranges from one probe read, so a mutation does not run `getent`
  twice; the connection loop sends every account request, including the list,
  to `spawn_blocking`, because every step reads `/etc` or shells out.
  `[accounts]` has two separate
  switches: `control` and `manage_passwords`, both off in release, and debug
  turns only `control` on.
  The tools come from `shadow` (`community`), not BusyBox, which has no
  `usermod`, `groupmod`, `chpasswd` or `gpasswd`; a missing tool answers
  `Unsupported` rather than guessing with different flags.
- **Frontend accounts**: `anth-account-manager` owns two panes (users and
  groups), sorting, filtering, the editors and confirmed mutations, and only
  depends on the injected `AccountClient`. `lib/accounts/account-client.ts`
  implements `/api/accounts` and `lib/accounts/display.ts` owns the pure
  helpers: labels, filtering, sorting, form parsing and `userBlockReason` /
  `groupBlockReason`. Those two mirror the daemon's guards so a row is disabled
  instead of offering an action certain to fail; they are a hint, never the
  policy. `locked: null` renders as `unknown`, never as `unlocked`, because the
  daemon could not read the shadow entry. The editors send only the fields that
  actually changed and refuse an empty modify with "Nothing changed". Native
  `required` is deliberately absent: the component validates and reports, so
  there is only one validation path. Tests live beside the component and
  `lib/accounts/`; the harness is
  `src/components/anth-account-manager.test.html`.
- **Frontend firewall**: `anth-firewall` reads the snapshot through an injected
  `FirewallClient`, shows managed rules, adds allow/deny rules and confirms
  deletes/flushes. It only renders controls when `control_enabled` is true and
  suppresses refreshes while a mutation is in flight. It shows the daemon's
  `warning` above the table and labels the count as `shown of total` when the
  snapshot was shortened. The dev harness is
  `src/components/anth-firewall.test.html`; tests live beside the component and
  `lib/firewall/`.
- **Volumes**: `volumes.rs` reads `lsblk --json --bytes` and
  `/proc/self/mountinfo`, flattens the device tree with `parent` links and
  reports **every** mount, virtual filesystems included. Mount and unmount are
  validated against a fresh snapshot: the device must exist and be unmounted,
  removable media needs `allow_removable`, the target must be an existing
  absolute directory inside `volumes.mount_root`, and an unmount may not cover
  `/`, a virtual filesystem, the socket, the config, `files.root` or the
  executable. `mount`/`umount` run through `command::SystemRunner` with an
  argument vector, never a shell; stderr text is classified into
  `VolumeErrorKind`. `fit_snapshot` mirrors `process.rs`: it drops diagnostic
  fields first, then device and mount tails, setting `total`, `truncated` and
  `warning`. Partitioning and `mkfs` are out of scope. Both mutations run on a
  blocking worker.
- **Frontend volume manager**: `anth-volume-manager` renders the device tree and
  the mount table through an injected `VolumeClient`
  (`lib/volumes/volume-client.ts`), with `lib/volumes/display.ts` owning the
  tree depth, filtering, sorting and usage helpers. Refresh is manual: there is
  no SSE stream, so the component never holds a connection open. Controls only
  appear when `control_enabled` is true, virtual filesystems never offer
  unmount, and the removable checkbox only appears for removable media. Tests
  live beside the component and `lib/volumes/`; the dev harness is
  `src/components/anth-volume-manager.test.html`.
- **Frontend auth**: `lib/auth/auth-client.ts` talks to `/api/auth/*`. The tokens
  are `HttpOnly` cookies, so the client never sees one; it reads the session,
  arms one silent refresh slightly before the access token expires, and shares a
  single in-flight refresh across callers because rotation would make the second
  one fail. `anth-login` is the form and reports success upward; `anth-app` owns
  the gate, showing the login when the session is gone and a sign-out button when
  auth is on. The shell takes an injectable `authClient`, which is how its tests
  avoid a server.
- **Frontend settings**: `anth-settings` renders `GET /api/config` and saves the
  sections that differ through `PATCH /api/config`, so a field changed by another
  tab is not overwritten. `lib/config/config-client.ts` owns the transport.
- **Frontend host info**: `anth-host-info` renders the `GET /api/host-info`
  snapshot and follows `/api/host-info/watch` through the injected
  `HostInfoClient`. It formats uptime, load, memory and swap, and exposes
  pause/resume, refresh and interval controls. `lib/hostinfo/display.ts` owns
  the pure formatting helpers; tests live beside both modules and the harness is
  `src/components/anth-host-info.test.html`.
- **Frontend desktop**: `anth-desktop` owns the open windows for the whole
  shell — one entry per panel, a global z order and the `localStorage`
  arrangement (`lib/ui/windows` holds the pure geometry and storage helpers).
  `anth-window` is controlled: it renders the `rect`, `z` and `minimized` it is
  given and reports `move`, `resize`, `focus`, `minimize`, `popout` and
  `close`. A minimized window is **detached** (`remove()`), not hidden, so its
  SSE stream and terminal WebSocket close while its state survives; restoring
  re-appends the same element. Opening a panel that is already open raises it
  instead of building a second copy, which would start a second stream against
  the same daemon. `#/window/<id>` is a route, so a pop-out is a URL behind the
  same session gate. The shell renders one desktop for every route, so a window
  outlives a navigation; the harness is `src/components/anth-desktop.test.html`.
- **Shared web helpers**: `lib/http.ts` owns `responseError` and the common
  fetch/EventSource option types, `lib/stream.ts` owns the `EventSource`
  lifecycle and JSON event parsing, and `lib/format.ts` owns `formatBytes` and
  `formatKib`. Clients and components import these instead of copying the same
  transport or formatting code.
- **Dev harnesses**: `src/components/component-index.test.html` links every
  harness. `src/harness/anth-harness.ts` provides the shared page shell
  (eyebrow, heading, `description`/`actions`/default slots), while
  `src/harness/harness.css` owns the common light-DOM controls, notes and page
  background. The `src/components/anth-*.test.html` files keep only their
  specific markup and wiring; none of this is a Vite build entry.
- **Terminal transport**: the daemon owns each PTY and the line protocol carries its
  bytes; `terminal/web.rs` maps the browser JSON frames to `Request`/`Response`, and
  `ws.rs` is the terminal-agnostic JSON/WebSocket wrapper. One browser WebSocket owns one
  daemon connection and one session; dropping either side kills the process group.
  An exited session is removed from the connection's `owned` list so the daemon returns
  to the socket-only path instead of polling an exhausted output stream;
  `tests/terminal_exit.rs` guards that CPU regression.
  Authentication is deliberately unimplemented: `terminal.token` is not checked yet.
- **Frontend terminal**: `anth-terminal` is an xterm-backed Lit element. It only
  depends on the `TerminalTransport` interface in `lib/terminal/transport.ts`;
  `lib/terminal/websocket-transport.ts` implements it for `/api/terminal`, and
  `lib/ws/json-websocket.ts` is the generic transport any future endpoint can reuse.
  xterm is loaded with dynamic `import()` so the initial bundle does not pay for it.
  The component's `keyboard-mode` defaults to `auto`: shells with line editing
  (bash, ash, zsh) handle Up/Down and Tab natively, while `dash` falls back to
  the component's local history/completion. The alternate buffer (btop, vim,
  htop) always keeps the program's arrows and Tab. The default height is sized
  for full-screen TUIs.
  `src/components/anth-terminal.test.html` is the dev harness and is not a
  Vite build entry.
- **Subcommands**: `daemon`, `api` and `run` share `--addr`, which falls back to
  `ANTH_PANEL_ADDR` and then `0.0.0.0:8080`. `run` binds the HTTP port before starting
  the daemon and aborts it when the API stops, which is what the container uses.
  `--config` is `global = true`, so it is accepted before or after the subcommand.
  Most client subcommands use `request_for` + `stream::follow`; `file read` and
  `file upload` own their socket loop because they move raw bytes in chunks.
  `network up` and `network down` ask for confirmation unless `--yes`.
  `firewall allow`, `deny`, `delete` and `flush` ask for confirmation unless
  `--yes`. `volume mount` and `volume unmount` do too.
  `stream::follow` turns a `VolumeError` into exit code `1`, like the other
  typed error responses.
- **Interactive sessions**: `-i`/`--interactive` is a global flag and
  `Command` is an `Option` because `-i` alone is valid; `main` rejects a bare
  invocation. `repl.rs` owns the loop (prompt, `help`, `quit`, empty lines,
  quoting) and takes a `Runner`, a boxed `FnMut(Command) -> Future<u8>`; `main`
  supplies one that calls `dispatch`, the same function a one-shot invocation
  runs, so a typed command cannot drift from the subcommand. `repl::session`
  takes a `Lines<R>` so tests drive it from a byte slice. `daemon`/`api`/`run`
  are detached by the dispatcher on a background task (the session owns a
  `BackgroundServers` registry of join handles) and awaited only until their
  port is bound, so the prompt stays usable; starting the same service twice is
  refused there because the second copy would steal the socket, and the registry
  is aborted when the prompt closes. `terminal` is still refused: it needs the
  stdin the line reader owns. A watch prints one frame instead of streaming.
- **Env vars**: `ANTH_PANEL_ADDR` (panel bind address, default `0.0.0.0:8080`),
  `ANTH_CONFIG` (config path, used by the Docker entrypoint), `RUST_LOG`.

## Documentation

User-facing docs live in `docs/` and are indexed from `README.md`. Keep them in
sync with behaviour; they are the contract for people writing clients:

- `docs/cli.md` — every subcommand and flag. Update when a flag is added.
- `docs/api-endpoints.md` — routes, request/response bodies, SSE fields, error
  shape, status codes. Update when a route is added or its payload changes.
- `docs/socket-protocol.md` — the `anth-proto` wire format. Update when either
  enum changes; an alternative client depends on this being accurate.
- `docs/terminal.md` — PTY sessions, browser route, current risks.
- `docs/processes.md` — process model, signal policy, truncation and controls.
- `docs/network.md` — network data sources, traffic counters, up/down policy.
- `docs/firewall.md` — managed nftables table, commands, API and container
  capabilities.
- `docs/volumes.md` — storage inventory, mount and unmount policy, privileges.
- `docs/accounts.md` — local user and group inventory, guard matrix, the
  `shadow-utils` dependency and the container's root requirement.

`docs/api-endpoints.md` also documents the session lifecycle and `PATCH
/api/config`, and `docs/cli.md` the `auth` and `config` commands.

`AGENTS.md` is for contributors and agents; `docs/` is for users. Do not
duplicate route tables here — point at the doc.

## Environment

- **Rust target is musl, always.** `backend/.cargo/config.toml` pins
  `x86_64-unknown-linux-musl`. Run a bare `cargo build` / `cargo test` /
  `cargo clippy`; never pass `--target x86_64-unknown-linux-gnu`. A sandbox that
  lost its toolchain needs `rustup target add x86_64-unknown-linux-musl` plus
  `apt-get install musl-tools` (for `musl-gcc`, which the C dependencies use).
  The host `x86_64-unknown-linux-gnu` std stays installed by rustup because
  proc-macros and build scripts compile for the host; that is a toolchain
  requirement, not a project target. Never build the project itself for gnu.
- **bun is the only package manager, and `bunx` does not exist.** Run scripts as
  `bun run <script>` from `frontend/`. `npm`, `npx` and `corepack` are removed on
  purpose; reinstalling one to "just run a script" breaks the rule.
- `tsc` is reached through `bun run tsc --noEmit`; there is no global `tsc`.

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
