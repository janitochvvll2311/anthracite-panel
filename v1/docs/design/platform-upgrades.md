# Platform upgrades

Status: phases 0–8g are done. This document is the plan of record and the
progress log for the multi-step upgrade of the panel. It is also the context
handoff: a later conversation should be able to continue from here without
re-reading the whole repository.

## Agreed decisions

| # | Decision |
| --- | --- |
| 1 | Order: foundation → utilities → auth → windows → processes → files → network → partitions → plugins |
| 2 | Auth bootstraps an `admin` account with a random password printed once; enabled in release too; tests build `Config` with `auth.enabled = false` |
| 3 | JWT covers the HTTP and WebSocket interface only; the Unix socket relies on file permissions |
| 4 | JWT is implemented in-tree (HS256 over `hmac` + `sha2` + `base64`) to keep the musl build free of C dependencies |
| 5 | The file editor uses CodeMirror 6, loaded with a dynamic `import()` |
| 6 | Archives: in-tree pure-Rust crates for `zip`, `tar`, `tar.gz`, `tar.xz` and `7z` extraction; no external archive binary. Creation is `zip`/`tar`/`tar.gz`/`tar.xz`; creating `.7z` is not offered. Superseded the earlier "`.xz` out of scope" wording — see phase 5 notes |
| 7 | Partitions use `sfdisk`; `mkfs` is supported behind its own switch |
| 8 | Recycle bin follows the freedesktop.org trash spec (`.Trash/$uid` or `.Trash-$uid` per volume, `$XDG_DATA_HOME/Trash` for the home volume), so deletions are restorable from the desktop too. Supersedes the earlier per-directory `.anth-trash/` |
| 9 | `bun` only; `npm` and `npx` are uninstalled |
| 10 | Renames drop the old custom element names, with no compatibility alias |
| 11 | UI source language is English; Spanish ships through `lit-localize`; all code, comments and docs stay in English |
| 12 | Passwords use PBKDF2-HMAC-SHA256 with a random salt, in-tree over `hmac` + `sha2` |
| 13 | `cookie_secure` defaults to `false`, which is what a loopback bind needs; a deployment behind TLS sets it to `true` |
| 14 | Process spawn is gated by a binary allowlist; an empty list denies every spawn |
| 15 | Network interfaces support create, edit, rename and delete, rename only while the link is down |
| 19 | Physical interfaces may be deleted. The confirmation says so explicitly ("this is a physical device; the host will lose this link") instead of hiding the action, and the delete still requires the typed interface name |
| 20 | Delete is refused for the interface the request arrived on (resolved from `peer_addr`), for any interface carrying the default route, and for the last interface carrying it. The peer guard is skipped when the address is a proxy or loopback, where the peer says nothing about the path; `[network] manage` stays `false` in release |
| 16 | Partitions support create, edit, resize and delete, plus `mkfs` behind its switch |
| 17 | A plugin bundle requires a valid Ed25519 signature by default: `plugin.sig` covers `plugin.wasm` and `[plugins] public_key` is the trust anchor. `[plugins] require_signature = false` is the explicit bypass for a development host or a publisher that has not signed yet, and every install through it prints a warning naming the skipped check. A signature that is present is always verified, bypass or not |
| 18 | Plugins can be installed from the web UI, not only from the CLI. The upload route is gated by `[plugins] install_from_web`, which defaults to `false` in release and `true` in the generated debug config, following the same rule as the other control switches. The UI must confirm a warning naming the code execution risk before it uploads the bundle |

Additional decisions taken while planning:

- Reviews: Copilot review plus a self review with the `code-review` skill. After
  two rounds that only raise minor findings, the remaining items are documented
  and the pull request is merged to keep moving.
- Review waits: after opening a pull request the agent waits up to two hours in
  fifteen-minute intervals. If no review arrives in that window, the next phase
  starts on a branch stacked on the unreviewed one instead of waiting further.
- Branches: `feat/<n>-<slug>` from `main`, one pull request per point, merged
  into `main` by the agent.
- `terminal.token` was removed once the global JWT middleware landed. The
  middleware protects `/api/terminal` like every other route, so the old token
  authorized nothing. Removing the key is a breaking config change: a file that
  still carries `[terminal] token` now fails `deny_unknown_fields`. Operators
  upgrading must delete the line; `docs/terminal.md` carries the migration note.
- Routing is hash based (`#/panel/<id>`).
- `mkfs` types: `ext4`, `xfs`, `vfat`, `exfat`, `btrfs`, `swap`, each behind
  `[volumes] format = false`.
- Plugins: `plugin.toml` manifest plus `plugin.wasm` and an optional
  `plugin.sig`, one plugin per zip. The runtime is the in-process `wasmi` host
  described in [plugins.md](plugins.md); there is no long-lived plugin process
  and no `Request::Plugin` on the wire.
- Plugin installation is reachable from two places: `plugin install PATH` on the
  CLI and `POST /api/plugins/install` from the web UI. Both end in the same
  `plugins::install_bundle` function, so the zip checks, the manifest
  validation, the hash and the signature verification exist once. The web route
  is gated by `[plugins] install_from_web`: off in release, on in the generated
  debug config, the same rule the process, network, firewall, volume and file
  switches follow. The debug default is paired with the signature requirement and
  the UI warning below; a debug build is a developer's own machine, and leaving
  the switch off there would mean the only untested path is the dangerous one.
- The web UI treats installation as a dangerous action. The install action is
  behind a confirmation that names the consequence, the panel says the plugin
  will run with the daemon's privileges, and the acknowledgement is per action
  rather than persisted — a stored "do not warn me again" would turn a
  deliberate confirmation into a default. `anth-modal` renders the warning; the
  harness uses the same flow. The warning is unconditional: it does not
  disappear because the switch happens to be on in a debug build.
- Trash layout: freedesktop.org Trash specification 1.0 — `<files>/` and
  `<info>/<name>.trashinfo`, chosen per volume, with a configurable retention
  size.
- Archives are extracted in-tree rather than by shelling out. `7z`, `p7zip` and
  `xz` are not in the runtime image, and Alpine's `tar` is BusyBox, whose
  escape behaviour could not be verified here; the extraction guard should not
  depend on an unverified binary.
- A deletion on a volume with no usable trash directory is refused rather than
  silently made permanent; the request may ask for a permanent delete
  explicitly.

## Phases

| Phase | Scope | State |
| --- | --- | --- |
| 0 | Handler-forward dispatch, `/api` 404, app shell, i18n rig, bun toolchain | done |
| 1 (point 5) | `anth-modal`, `anth-dialog`, `anth-tab-panel`, harness migration | done, merged in #16 |
| 2 (point 7) | JWT auth, refresh rotation, login screen, config commands and settings harness | done, merged in #17 |
| 3 (point 6) | Floating windows, minimize, standalone window | done, merged in #20 |
| 4 (point 1) | `anth-process-manager` and process spawning | done, merged in #21 |
| 5 (point 2) | `anth-file-manager`: CodeMirror editor, archives, recycle bin | done, merged in #22 |
| 6 (point 3) | `anth-network-manager`: interface create/edit/rename/delete | done, merged in #23 |
| 7a (point 4) | Partition create/edit/resize/delete | done, merged in #28 |
| 7b (point 4) | `mkfs` for new and reshaped filesystems, behind `[volumes] format` | done, merged in #28 |
| 8a (point 8) | Plugin manifest, typed validators, binary allowlist, install/list/remove/approve, grants in config, hash re-verification | done, merged in #24 |
| 8b (point 8) | `wasmi` host, ABI, fuel and memory limits, blocking execution, WAT test guest | done, merged in #25 |
| 8c (point 8) | API anchors and namespacing | done, merged in #26 |
| 8d (point 8) | UI anchors and asset serving | done, merged in #29 |
| 8e (point 8) | WireGuard plugin: manifest, wasm, web components | done, merged in #30 |
| 8f (point 8) | Ed25519 bundle signatures, the `require_signature` bypass, browser install and `anth-plugin-manager` | done |
| 8g (point 8) | Per-plugin grant files, dynamic Network plugin tabs, host jobs and persistent WireGuard | done |

## Phase 8 — plugins (done)

The reference bundle closed in #30; 8f then added signatures and the browser
install path. The first review of the bundle found four defects that the suite
could not see, which is the useful lesson of the phase rather than the plugin
itself:

- The guest read an anchor body as an object while the host forwards it as a
  JSON string, so the only mutating route answered `400` to every real request.
  The suite stayed green because the test hand-built the envelope.
- `fixture!` returned early when the guest was unbuilt, so on a fresh clone
  seven tests asserted nothing while `cargo test` reported success.
- `plugin.toml` declared `panel.network.tab` / `anth-wireguard-panel`, a tag no
  module registered and a slot no component mounts.

All three were shapes of the same failure: a test that encodes an assumption
instead of the contract. The fixes drive the route through the real router,
build the guest when it is missing or stale, and assert every declared tag is
defined by the module it names. `docs/design/plugins.md` now states which slots
are mounted instead of implying that declaring one is enough.

## Phase 5 — file manager (done)

Split into four commits so each one is reviewable on its own:

1. Recycle bin (freedesktop trash). Done.
2. Archive create/extract/list, with the extraction guards. Done.
3. Text editing endpoints, then the CodeMirror component. Done.
4. Rename `anth-file-explorer` to `anth-file-manager`, harness and docs. Done.

### Text editing — what was decided while implementing

- Saving is a new endpoint, not `upload`. Upload replaces a file by name and
  knows nothing about what the editor loaded; an edit has to refuse when the
  file moved underneath it, and that needs a precondition.
- The precondition is the mtime from the listing, which the client already
  receives. Passing it back as `expected_modified_ms` costs one field and needs
  no server-side session or token per open buffer.
- A refused save must leave the editor dirty. The text is still only in the
  buffer, so labelling it "saved" would misstate where the data lives.
- The editor writes through a symlink instead of replacing it. Renaming a
  temporary file over a link would detach a linked config file, which is a
  silent behaviour change for an "edit".
- CodeMirror is loaded with `import()` behind the four-method `EditorHandle`.
  The component then has one seam for tests, and the initial bundle does not
  carry eight grammars.
- Formats without a grammar the panel ships (`shell`, `toml`, plain text) keep
  line numbers and bracket matching rather than claiming to be highlighted.
  Word autocompletion from the document covers them all.

### Recycle bin — what was decided while implementing

- The spec keys the home trash on the *device* holding `$HOME`, not on the path
  being equal to `$HOME`. Comparing paths looked right on a desktop and was
  wrong in a container, where `/`, `/tmp` and `/home` are one overlay; the first
  end-to-end run picked `/.Trash-10001` and failed with `EACCES`. A test now
  pins the device rule.
- A volume with no writable trash directory is an error naming the directory,
  not a silent permanent delete.
- `DeletionDate` is written in UTC. The spec asks for local time and the daemon
  has no timezone database; readers only display the value, so a valid UTC stamp
  beats a stamp that is wrong for half the year.
- `.trashinfo` values are parsed strictly: a malformed percent escape or a
  missing `Path` is skipped, never guessed at. `Path` is untrusted input on
  restore and goes through `normalize` plus the root resolution.
- Retention (`[files] max_trash_bytes`, default 1 GiB) is best effort and runs
  after the move, so it can never fail a delete that already happened.

### Archives — what was decided while implementing

- The crates are the boundary. Delegating to `tar`/`xz`/`7z` would put the
  security check in a binary whose escape handling differs between GNU tar and
  BusyBox, and the runtime image has BusyBox.
- `.7z` creation is refused. `sevenz-rust2` exposes no control over the filter
  chain, so the output would not be what users expect of a `.7z`; reading is
  fully supported.
- The extractor refuses a hostile member instead of skipping it. A partial tree
  that looks complete is worse than an error.
- Backslashes are rejected outright. They are ordinary filename characters on
  Unix, so accepting them creates a tree that becomes an escape when copied to
  Windows.
- The end-to-end run caught two bugs the unit tests missed: `docs.zip` extracted
  into `docs` produced `docs/docs/`, and a zip directory entry rendered as
  `docs//`. Both now have regression tests. The strip is conditional on the
  archive's single common root matching the destination's own name, so an
  unrelated destination keeps the stored layout.
- Interop is asserted against system `tar` and Python's `zipfile` rather than
  against our own reader, so a writer bug cannot certify itself.

## Phase 4 — process management (done)

What changed:

- `process.rs` replaced the `control: bool` threading with `ProcessPolicy`,
  resolved once per request from `[process]`. `collect`, `collect_from`,
  `send_signal` and `spawn` all read it, so a caller cannot pass the switches in
  an inconsistent order.
- Spawning is a second switch, `[process] spawn`, plus `spawn_allow`. The list,
  not the switch, is the authority: an empty list denies every request even with
  `spawn = true`.
- Validation canonicalises before comparing. `/bin/echo` and `/usr/bin/echo` are
  the same file on a merged-usr host, so a text compare would reject a program
  the operator did list; canonicalising also means a symlink cannot point outside
  the allowlist.
- The child gets `Stdio::null()` on all three streams and is reaped by a thread
  per child. Without the reap every short-lived spawn left a zombie; using a
  thread avoids touching the global `SIGCHLD` disposition, which the PTY
  sessions also depend on.
- The protocol gained `ProcessSpawn`, `ProcessStarted` and `ProcessSpawnError`
  with a machine-readable `ProcessSpawnErrorKind`, so a client branches on the
  kind and never parses the message.
- `POST /api/processes/spawn` maps the kind to a status: one `403` class for
  every "not allowed" refusal, `404` for a missing program or directory, `400`
  for a malformed body. The handler does not re-check the policy; asking twice
  is how two copies drift apart.
- The component was renamed `anth-process-monitor` → `anth-process-manager`
  with no compatibility alias. It gained a spawn modal that only opens when the
  snapshot says spawning is possible, an allowlist chip row, and the same
  confirm-then-act flow the signal button already used.
- `parseSpawnForm` validates before the request leaves the browser; the daemon
  still decides.
- The child calls `setsid()` in `pre_exec`, so it leads its own session and
  process group. Only the group-signal half is claimed: `setsid` changes the
  session, not the cgroup, so a cgroup-scoped stop still kills the child.

Bug found and fixed while doing this: `anth-settings` diffed its draft with
`!==`, so the new `spawn_allow` array always looked edited and a save with no
changes still sent a `PATCH`. Array fields now compare element by element.

Review found three more, each now covered by a test that fails without the fix:

1. `spawn_allow` was serialised with `skip_serializing_if = "Vec::is_empty"`.
   An empty list is the documented deny-all state, so omitting it left the
   browser reading `undefined` and throwing on `.length` — which broke the whole
   process panel in the default release config, not just the spawn form. The
   field is always sent, and the client treats absence as empty.
2. `pre_exec` never called `setsid()`, so the documented "own process group"
   was false and a group signal took spawned children down with the daemon.
3. The first version of that test spawned `--list`, which exits before `/proc`
   can be read, so it failed about one run in ten under load. It now spawns
   `sleep 30`, samples, and kills before asserting.

Verification: 558 backend tests, 425 frontend tests, `tsc --noEmit`,
`vite build`, `cargo fmt --check` and `cargo clippy --all-targets` clean, plus a
live daemon for the spawn refusal paths, the process group and the auth gate.

## Phase 0 — foundation (done)

What changed:

- `backend/anth-panel/src/handler.rs` is new. `Handler` has `handle` (returns
  `None` when it does not recognise the request) and `blocks` (whether the
  request may block). `dispatch` walks the chain and answers a request no
  handler accepted with a readable error. `CoreHandler` holds every command the
  panel implements itself.
- `daemon.rs` no longer keeps a second `match` over the request set. The
  connection loop owns the stateful requests (uploads and terminal sessions) and
  asks the chain for everything else, including whether it needs
  `spawn_blocking`. Adding a command now touches the protocol, the client, the
  CLI and one handler arm.
- Unknown paths under `/api` answer `404 {"error": ...}` instead of the SPA's
  `index.html`, so a route typo fails at the status code instead of at the JSON
  parse.
- The frontend got its app shell: `src/app/anth-app.ts` (header, home page of
  cards, panel host, language selector), `src/app/panels.ts` (the registry the
  shell renders), `src/app/router.ts` (pure hash routing) and `src/app/i18n.ts`
  (idempotent `lit-localize` runtime configuration). `index.html` mounts the
  shell; the Vite scaffold component is gone. The later tab-based layout is
  described in [app-shell.md](app-shell.md).
- The shell injects clients through a `clientFactory` property, so tests mount
  every panel against the existing fake clients and no test opens a socket.
- `lit-localize` is wired: `bun run i18n:extract` writes `frontend/xliff/es.xlf`
  and `bun run build` regenerates `src/generated/locales/es.ts` before type
  checking and bundling.
- Toolchain: `bun` 1.4.3 replaces `npm`, `npx` and `corepack`. `bun install`,
  `bun run test` and `bun run build` are the supported commands.

Verification: 448 backend tests, 289 frontend tests, `cargo fmt --check`,
`cargo clippy --workspace --all-targets`, `bun run build`, and a real
`anth-panel run` serving the embedded UI, a live process table and the Spanish
locale.

Notes for the next phase:

- The panel registry is the place to declare a new panel; the shell needs no
  change. The window manager in phase 3 will render the same entries.
- `AppState` still copies the configuration. Phase 2 turns it into
  `Arc<RwLock<Config>>` when the config endpoints land.
- The handler chain has room for a plugin link; phase 8 appends it after
  `CoreHandler`.

## Phase 1 — UI utilities (done)

What changed:

- `lib/ui/dialog.ts` exposes `confirmAction`, `promptText` and `showAlert` as
  promises routed through one `anth-dialog-host`. One dialog is visible at a
  time and a second request queues behind it, because two stacked confirmations
  from one click is a caller bug the operator cannot answer.
- `anth-modal` is a thin wrapper over the native `<dialog>` element, which
  already supplies the top layer, the focus trap and `Escape`. It never closes
  itself: it reports `escape`, `backdrop`, `close` or `programmatic` and the
  owner decides, so a dialog with unsaved input can refuse.
- Every panel component now asks through those helpers instead of
  `window.confirm`/`prompt`/`alert`, which block the event loop and freeze a
  live terminal session.
- `anth-tab-panel` parks a hidden tab with `remove()` rather than
  `display: none`. A detached custom element keeps its properties, its shadow
  root and its internal state, so a tab resumes where it left off while its SSE
  stream and WebSocket are closed.
- Each tab is observed individually, because a parked tab is detached and an
  observer on the panel only watches descendants. `connectedCallback`
  re-observes every known tab: `registerTabs` returns early for an id it already
  knows, so a panel that is moved or re-appended would otherwise watch nothing.
- Restoring a tab changes no property, so `willUpdate` never runs in the child.
  The panel dispatches `anth-tab-show` on the element it puts back, and
  `lib/ui/tab-visibility` is the shared subscription helper. The process,
  network and host-info monitors start their stream from it, but only when the
  stream was live when the tab was parked: a stream the operator paused with the
  Pause button is not resurrected by a tab switch.
- The dialog host restores focus to the opener once the last dialog of a burst
  is answered. The frame is removed while its native `<dialog>` is still open,
  so the browser's own restoration never runs.

Dev harnesses: `anth-dialog.test.html` and `anth-tab-panel.test.html`, both
linked from `component-index.test.html`.

Verification: 326 frontend tests, `tsc --noEmit`, and the dialog and tab
harnesses checked in a real browser.

Lessons for later phases:

- A component that owns a connection and lives inside a tab must start it from
  `connectedCallback` (or from `onTabShow`) as well as from `willUpdate`, and
  must release the listener in `disconnectedCallback`.
- `bun` can disappear from `PATH` after a shell restart. The local binaries
  under `frontend/node_modules/.bin` are the reliable way to run `tsc` and
  `vitest`; `npm` is uninstalled and must not be used.

## Phase 2 — Authentication and configuration (done)

What changed:

- `anth_panel::auth` is a module tree rather than a middleware with helpers
  inside it: `password` (PBKDF2-HMAC-SHA256), `jwt` (HS256 only), `session`
  (refresh store and `AuthService`), `cookies` (attributes and parsing),
  `middleware` (the gate) and `routes` (the five endpoints). Each has its own
  unit tests, so a change to the digest format or the token claims fails next to
  the code that produced it rather than in an HTTP test.
- `jwt.rs` accepts one algorithm. A parser that honours the token's own `alg`
  header is the classic JWT bug: `alg: none` or a swapped RS256 turns
  verification off. The header is read, compared and ignored otherwise.
- Refresh tokens rotate. Every login stores a `jti`; a refresh revokes the one
  it presented and issues a new pair. Presenting an already-revoked `jti`
  cancels the session instead of answering `401` once, because reuse is what a
  stolen cookie looks like.
- The layer protects the whole router in one place (`protect`), so a new route
  is closed by default and the three login routes carry an explicit public
  marker. Adding per-route `#[layer]` calls is how one endpoint ends up
  unprotected after a refactor.
- The middleware accepts the cookie or `Authorization: Bearer`, because the UI
  keeps cookies and a script has neither a browser nor a cookie jar. Both paths
  verify the same token and reach the same claims.
- `AppState` became `Arc<RwLock<Config>>` plus `Arc<RwLock<SharedAuth>>`, with
  `config().await` and `auth().await` accessors. `PATCH /api/config` needs to
  change both at runtime, and a copied `Config` cannot express that.
- A patch is validated by serializing it and parsing it back before it is
  stored. The alternative is discovering the file is unloadable on the next
  restart, when the panel no longer starts.
- `PATCH` refuses `auth.enabled = false`. The panel is exactly the place a
  stolen session would go to remove the lock, so turning auth off stays a host
  command.
- First run with auth on generates a password, prints it once to the terminal
  and stores only the digest, plus a generated signing key. A panel that starts
  with an empty password hash is an open shell, so the default has to be a
  usable secret rather than a disabled feature.
- `auth` and `config` subcommands edit the file directly. The password is read
  with echo off from `/dev/tty` (or stdin with `--stdin`), never assembled from
  an argument, so it does not appear in `ps` or in shell history.
- The frontend `AuthClient` owns the silent refresh and shares one in-flight
  request. Rotation makes a second concurrent refresh fail, and a page with
  several panels would otherwise log itself out as soon as two streams refreshed
  at once.
- `anth-app` is the gate: `unknown` until the session read finishes, then the
  login or the panels. Rendering nothing in between is what prevents a flash of
  protected content.
- `anth-settings` diffs the draft against the loaded view and sends only the
  changed sections. A full replacement would overwrite a field another tab
  changed while the form was open.

Two review rounds followed. The second one reopened one of the first round's
fixes and caught two bugs that round had missed. All six are closed:

- **Regression.** Rotation had a check-then-act window: two concurrent refreshes
  of one token both got a pair. `7bede34` held one `sessions.write()` across the
  check and the spend, keyed by `jti`; `fc40e81` dropped the lock before
  re-reading the session by `sid`. The check, the spend and the re-issue now
  happen in one write-lock section.
- **Missed in round one.** Access tokens were verified by signature alone, so a
  password change left one working for its full lifetime. Every token now
  carries a `sid`, and `verify_access` asks the session store whether that
  session is live. A password change rotates the signing key and rebuilds the
  service, so the old token dies at once rather than at expiry.
- **Missed in round one.** A refresh token was accepted as an API credential.
  Tokens carry a `kind`, and only `access` reaches a route, so a leaked refresh
  token is good for one rotation and nothing else.
- `replace_config` rebuilt the auth service, which signed the operator out on a
  settings save. Policy fields now sit behind an inner lock and are applied in
  place; a credential change uses the separate `replace_auth`.
- `access_expires_in` and `refresh_expires_in` reported the configured TTL
  instead of the time left, so a tab loaded mid-token scheduled its silent
  refresh too late. They are computed from each token's `exp`.
- The config file was world-readable; it holds the digest and the signing key.
  `Config::save` writes it `0600`.

Verification: backend suite green including 27 router-level auth tests, 348
frontend tests, `tsc --noEmit`, `cargo fmt --check` and `cargo clippy` clean.
Harnesses: `anth-login.test.html` and `anth-settings.test.html`.

Lessons for later phases:

- Anything that adds a route inherits the session gate; a public route needs a
  deliberate exemption, never a missing layer.
- A component that reads a stream must cope with a `401` mid-stream. The
  refresh keeps the cookie alive, but a revoked session ends the stream and the
  shell has to take the operator back to the login.
- The daemon socket is still unauthenticated. Panel auth is a browser boundary;
  do not describe it as protecting the socket.
- A token must say what it is for, not only who it belongs to. "Signature valid
  and session live" was true of both halves of the pair, and that made a refresh
  token a 12-hour API credential.
- Check-and-act on shared state belongs in one lock section. The rotation bug was
  invisible to a single-threaded test and showed up under eight.
- A countdown in an API response has to be a countdown. Reporting the configured
  TTL is the kind of thing the writer of the endpoint cannot notice and every
  client gets wrong.
- A retry belongs next to the transport, but registering the client is what
  wires it. Seven panel clients pass `fetchImpl: currentApiFetch()` in
  `frontend/src/app/panels.ts` (host-info, processes, files, network, firewall,
  volumes, accounts); a new `PANELS` entry must do the same or the client falls
  back to the bare `globalThis.fetch`. `HttpConfigClient` and `AuthClient`
  build their own fetch, and the `watch` streams go through `watchJson` and an
  `EventSource`, which never reach `withRetry`. A stream reader still has to
  handle a `401` itself.

## Phase 3 — Floating windows (done)

What changed:

- `lib/ui/windows.ts` is the layout model and nothing else: `Rect`, the
  `MIN_WIDTH`/`MIN_HEIGHT` floor, `dragRect`, `resizeRect`, `clampRect`,
  `fitToViewport`, `initialRect` (a diagonal cascade so two windows do not land
  on top of each other), `nextZ` and the `localStorage` read/write pair. Pure
  functions, so the edge behaviour is tested without a browser drag.
- `anth-window` is a controlled component: it renders the `rect`, `z` and
  `minimized` it is given and reports `move`, `resize`, `focus`, `minimize`,
  `popout` and `close`. The shell owns the layout because the z order is
  global, which a per-window counter cannot express, and because the
  arrangement has to survive a reload.
- A press on a title-bar button does not start a drag. The buttons stop the
  pointer event, so the drag handler never sees it; without that, releasing
  over a button after a small hand tremor would both move the window and fire
  the button.
- `clampRect` keeps the title bar on screen rather than the whole rectangle.
  Forcing the full box inside the viewport would make a tall terminal window
  impossible to shrink, and the point of the clamp is only that the grab handle
  stays reachable.
- **Minimizing detaches the panel instead of hiding it.** `remove()` on a
  connected custom element keeps its properties, shadow root and state, while
  dropping its SSE stream and its terminal WebSocket, because those live in
  `disconnectedCallback`. Restoring re-appends the same instance. The
  alternative, `display: none`, keeps every stream open and is exactly the cost
  the minimize exists to avoid. `anth-tab-panel` already used this technique;
  the desktop reuses it for a different reason.
- **One desktop, not one per panel.** The first draft rendered an `anth-desktop`
  per panel route and passed `initialPanel`, which meant a window could never
  outlive a navigation and two windows could never be open at once. The shell
  now renders a single desktop with every registered panel, and navigating
  opens a window in it. Opening a panel that is already open raises it instead
  of building a second copy, which would have started a second polling stream
  against the same daemon.
- The toolbar's "Open all in windows" became a panel picker. Eight windows at
  once covers the screen and buries the one the operator wanted; a `select`
  that opens exactly one panel is the useful action.
- `#/window/<panel>` is a real route, so a pop-out is a URL rather than a
  `window.open` with a document written into it. It is behind the same session
  gate as the rest of the shell: a pop-out must not become a way to skip the
  login, and an expired session there returns to the login screen like any tab.
- The deep-link case needed `firstUpdated`. A page loaded directly at
  `#/panel/<id>` arrives with the route already parsed, so no `route` change
  ever fires; without the first-render call the arrangement came back empty.

> Superseded by the tab-based shell in [app-shell.md](app-shell.md): the shell
> no longer renders `anth-desktop`, and `#/window/<id>` is an unknown page. The
> component, route parser and tests remain in the tree as utilities for a
> future window mode.

Verification: 402 frontend tests, `tsc --noEmit`, `vite build`, and the
`anth-desktop.test.html` harness in a real browser: open, drag, resize, minimize
(panel timer stops), restore in place, raise by click, reload to restore the
arrangement, and pop-out to `#/window/<id>`. A live `anth-panel run` served the
embedded UI and two windows coexisting across a navigation.

Lessons for later phases:

- A `@property()` without `attribute:` does not read the attribute of the same
  name. The minimize button silently did nothing until `window-id` was declared
  explicitly; the unit tests passed because they set the property directly.
- A layout that is persisted per storage key must be keyed to the thing that
  owns it. Two desktops sharing a key would fight over the same geometry.
- A controlled component that is also the drag surface must handle
  `pointercancel`, not only `pointerup`: a drag that ends outside the window
  otherwise stays "active" and follows the pointer on the next click.

## Phase 8 — plugins

Architecture is settled in [plugins.md](plugins.md): the daemon is the chef, the
manifest is the recipe, the plugin is a helper that only supplies declarative
values. The load-bearing rule is that a plugin never sends an argv; the host
renders it from a template frozen at install, using validators the host owns.

Runtime is `wasmi`, chosen over `wasmtime` because it is pure Rust: no JIT, no
`unsafe`, no WASI, and the static musl build stays trivial. A module therefore
has no filesystem, clock or randomness unless the host grants it. Cost is
interpreted speed, which is irrelevant for business logic.

Grants live in the config rather than a state file, because the admin already
reads and edits the config, and `plugin list` can audit authority from one place.

### 8a — authority boundary (done, merged in #24)

`plugins/manifest.rs` holds the schema and the validators, `plugins/bundle.rs`
the archive side and the store, `plugins/mod.rs` the config and the registry.

- `Manifest::parse` validates against host policy before anything is written:
  reverse-DNS id, matching ABI, a 64-hex module hash, capabilities whose program
  is on `PROGRAM_ALLOWLIST` or whose `job` is one the host implements, exactly
  one of the two per capability, placeholders that each have a validator, and no
  declared argument the template never uses.
- `render_argv` is the only way a capability becomes an argument vector. It
  refuses an undeclared argument, a value failing its validator, a missing
  value, and free text for a capability that did not set `accepts_text`.
- Every validator rejects a leading `-` in the shared prelude, because an
  operand that looks like an option is read as one: `wg show --force` is the
  flag a plugin was not granted. `plugin_dir` validates the id before joining,
  so an id such as `../victim` cannot reach a recursive delete.
- `install` extracts to a staging directory, validates, hashes, then renames.
  Every member must be `plugin.toml`, `plugin.wasm` or under `web/`; the guard
  reuses `files::archive::safe_member` and opens with `O_NOFOLLOW`. A failure
  leaves nothing behind.
- `load` hashes the module on every start and compares it against
  **`grant.wasm_sha256`**, not against the manifest's own hash: a manifest can
  be rewritten alongside a rewritten module, so comparing against it would keep
  the pair consistent while the grant still names the old digest. It also
  compares the version, and takes the advertised capability list from the grant
  rather than the manifest. The module path is
  pinned to `plugin.wasm` instead of being read from the manifest, so rewriting
  the manifest cannot re-point the file a grant was hashed against.
- `plugin list|install|remove|enable|approve|call` run locally, like `auth` and
  `config`, and say a restart is needed (except `call`, which runs at once). `approve` needs the explicit
  `--accept-capabilities` flag: widening a grant is never implicit.
- `GET /api/plugins` is read-only and served from the bundle directory, so the
  daemon stays unaware of which plugins the panel shows.

Tests: 59 unit cases (schema, validators, argv rendering, tampering, archive
guards) and 7 integration cases driving the real binary.

## 8b: the host

The grant now freezes the **recipe**, not only the id. That is the change this
phase forced: `invoke` renders argv from the grant, so a grant holding ids alone
would have left the program and template to be read from the manifest — the file
an attacker rewrites. `PluginGrant` carries `recipes: Vec<GrantedCapability>`,
`allowed_ids()` is the view the CLI and the panel read, and `recipe(id)` is what
execution uses.

`runtime::invoke` runs one envelope against a `wasmi` engine with `consume_fuel`,
a `StoreLimits` memory ceiling and exactly two imports (`anth_exec`, `anth_log`).
The four documented exports are checked by name and signature, and the ABI
version is compared before any request is written. `anth_exec` reads the recipe
from the grant, refuses host `job`s, validates every argument with the host's own
validators, and renders the argv itself.

`install` and `load` now also parse the module (`runtime::probe`). A digest proves
the bytes are the approved ones; it says nothing about whether they load.

`plugin call <id> [REQUEST]` is the CLI anchor and the manual entry point.

Tests: 13 runtime cases against a real `guest.wat` guest, including a module that
loops until fuel runs out, a guest that smuggles `--force` through a value, and a
guest importing WASI.

Next: 8c. API anchors under `/api/plugins/<id>/`, with the same `invoke` call
behind an axum handler and namespacing enforced at install.

### 8c — what the anchor route turned out to be (done, merged in #26)

One wildcard, five methods, and a handler that never reads the manifest. That
last part is the decision this phase forced. Declaring one axum route per anchor
would have meant consulting `plugin.toml` on every request to decide whether a
path is reachable — and `plugin.toml` lives inside the bundle, so the plugin
would have been choosing which of its own routes exist. The grant is the only
authority that survives a rewritten bundle, so the request goes to the guest and
the guest reaches the host through `anth_exec`, which reads the grant.

`anchor_route` still validates declared paths at parse, for a narrower reason: a
manifest naming `/api/auth/login` is either a mistake or an attempt, and an
operator should hear about it at install rather than see a plugin advertised as
offering a route that does not exist. That check is honesty, not authorization,
and the docs say so in both places a reader might look.

The envelope carries `body` as a **string**, not as parsed JSON: an anchor is not
required to speak JSON, and the guest knows its own shape. The guest answers with
`{"status": n, "body": ...}`; anything else is passed through as `200
text/plain` rather than guessed at. A guest failure is `502` — the plugin is
broken — and only fuel or the deadline are `504`; blaming the caller for a
decision it did not make would be the wrong signal.

Tests: 13 in `tests/anchors.rs` against real guests and a fake `wg` on `PATH`,
including a request for a path the manifest never declared (it must reach the
guest), an id of `%2e%2e`, an oversized body, the disabled switch, and a
manifest rewritten on disk after install.

## Phase 8f — signed bundles and browser install

Signature verification is part of the install boundary, not a policy the daemon
re-checks at run time. `plugin.sig` is a detached Ed25519 signature over
`plugin.wasm`; `[plugins] public_key` is the only key the host trusts. The
manifest's `sha256` is still checked, but it is not the trust anchor: an attacker
who rewrites both `plugin.toml` and `plugin.wasm` can keep that pair consistent,
while they cannot produce a signature without the private key. `load` continues
to re-hash against the frozen grant, so a module replaced after install is still
caught.

`[plugins] require_signature` defaults to `true`. Turning it off is the escape
hatch for a development host or an unsigned publisher; install then returns a
warning instead of failing, and the CLI and the plugin manager both print it. A
signature that *is* present is always verified, even with the bypass: the flag
allows an unsigned bundle, not a wrong one. `plugin approve` applies the same
rule, because approving is the other path that turns a bundle into authority.

`POST /api/plugins/install` takes the zip as the raw request body, caps it at the
bundle limit, installs on a blocking worker, writes the grant through
`AppState::replace_config` and persists the config. `install_from_web` gates the
route, `plugins.enabled` gates the rest, and a failed install keeps the previous
config. `anth-plugin-manager` lists the catalogue and its problems, disables the
form with the reason when either switch is off, and asks for confirmation with
the code-execution consequence before uploading.
