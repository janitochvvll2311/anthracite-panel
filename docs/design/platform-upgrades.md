# Platform upgrades

Status: in progress. This document is the plan of record and the progress log
for the multi-step upgrade of the panel. It is also the context handoff: a later
conversation should be able to continue from here without re-reading the whole
repository.

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
| 16 | Partitions support create, edit, resize and delete, plus `mkfs` behind its switch |
| 17 | A plugin manifest requires a binary `sha256`; a signature is optional and its absence is warned about, not rejected |

Additional decisions taken while planning:

- Reviews: Copilot review plus a self review with the `code-review` skill. After
  two rounds that only raise minor findings, the remaining items are documented
  and the pull request is merged to keep moving.
- Review waits: after opening a pull request the agent waits up to two hours in
  fifteen-minute intervals. If no review arrives in that window, the next phase
  starts on a branch stacked on the unreviewed one instead of waiting further.
- Branches: `feat/<n>-<slug>` from `main`, one pull request per point, merged
  into `main` by the agent.
- `terminal.token` is removed once the global JWT middleware lands; it stays as
  an accepted-but-ignored key until then.
- Routing is hash based (`#/panel/<id>`).
- `mkfs` types: `ext4`, `xfs`, `vfat`, `exfat`, `btrfs`, `swap`, each behind
  `[volumes] format = false`.
- Plugins: `plugin.toml` manifest, one long-lived JSON-lines process per plugin
  binary, one plugin per zip, binary must match the host architecture. Plugins
  never extend the `anth-proto` enums; they receive `Request::Plugin`.
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
| 3 (point 6) | Floating windows, minimize, standalone window | done, in review |
| 4 (point 1) | `anth-process-manager` and process spawning | done, merged in #21 |
| 5 (point 2) | `anth-file-manager`: CodeMirror editor, archives, recycle bin | in progress: recycle bin landed |
| 6 (point 3) | `anth-network-manager`: interface create/edit/delete | not started |
| 7 (point 4) | Partition create/edit/resize/delete and `mkfs` | not started |
| 8 (point 8) | Plugin system and the WireGuard plugin | not started |

## Phase 5 — file manager (in progress)

Split into four commits so each one is reviewable on its own:

1. Recycle bin (freedesktop trash). Done.
2. Archive create/extract/list, with the extraction guards.
3. Text editing endpoints, then the CodeMirror component.
4. Rename `anth-file-explorer` to `anth-file-manager`, harness and docs.

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
- The frontend has an app shell: `src/app/anth-app.ts` (header, home page of
  cards, panel host, language selector), `src/app/panels.ts` (the registry the
  shell renders), `src/app/router.ts` (pure hash routing) and `src/app/i18n.ts`
  (idempotent `lit-localize` runtime configuration). `index.html` now mounts the
  shell; the Vite scaffold component is gone.
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
