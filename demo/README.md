# Demo instance

The v0.6.0 demo runs the deterministic host emulator as eight service apps:
`org.anthracite.fs`, `terminal`, `proc`, `net`, `firewall`, `storage`,
`accounts` and `home`. Each bundle carries the future real app's WIT package
and id, and all eight share one native binary. Home renders the host overview and replaces Hello
in the composed profile, so the demo never reads the real machine's facts.
`apps/hello` stays in the tree as the minimal developer example and remains
installable.

The engine is a debug build here, so `[services] allow-third-party` defaults on
and the unsigned bundles are accepted. A release build keeps both closed.

The emulator claims the future first-party ids and WIT packages (`org.anthracite.fs`,
`anthracite:fs`, …). In a release those bundles are `system`-signed; in the demo
they are third-party and run under the explicit developer switch.

## Build the bundles

```bash
# Only needed after editing any frontend; the generated modules are committed
# so the bundle build itself does not need Bun.
apps/build-frontends.sh

cargo build --manifest-path demo/emulator/Cargo.toml --release
cargo run --manifest-path demo/emulator/Cargo.toml --release --bin bundle-emulator -- \
    --binary target/release/host-emulator --out demo/dist
```

`demo/dist/{fs,terminal,proc,net,firewall,storage,accounts,home}.anth` are the
installable zips. The app sources (manifest, WIT and frontend) live in
`apps/<name>/`; the demo builder packages all eight apps with the shared
`demo/emulator` service binary. Home's real backend stays in `apps/home/backend`; `apps/home/build.sh`
packages it for a machine that should show its own host facts.

## Run

From the repository root:

```bash
cargo run --manifest-path panel/backend/Cargo.toml -- \
    --config demo/config.toml --listen 127.0.0.1:5173
```

The first boot writes `demo/state/` and creates the operator account. In another
terminal, using the CLI:

```bash
ANTH="cargo run --quiet --manifest-path sdk/rust/Cargo.toml -p anth-cli -- --socket demo/state/engine.sock"
for app in fs terminal proc net firewall storage accounts; do
    $ANTH app install demo/dist/$app.anth
    $ANTH app approve org.anthracite.$app --accept-write --accept-admin
done
$ANTH app install demo/dist/home.anth
$ANTH app approve org.anthracite.home --accept-write --accept-admin --accept-host
$ANTH compose --add org.anthracite.fs --add org.anthracite.terminal \
    --add org.anthracite.proc --add org.anthracite.net \
    --add org.anthracite.firewall --add org.anthracite.storage \
    --add org.anthracite.accounts \
    --add org.anthracite.home --remove org.anthracite.hello --now
```

A v0.5 engine recomposes on the next restart; restart the panel after `compose`.

Then, from the panel session (or the CLI), the app routes answer:

```bash
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.fs/files?path=/
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.proc/processes
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.net/interfaces
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.firewall/snapshot
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.storage/mounts
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.accounts/users
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.home/info
```

CLI verbs:

- `fs files`, `fs file-stat`, `fs file-read`, `fs trash`, `fs rename`, `fs restore`
- `terminal sessions`, `terminal session-open`, `terminal session-read`, `terminal session-close`
- `proc processes`, `proc process-signal`, `proc process-spawn`
- `net interfaces`, `net routes`, `net interface-up`, `net route-add`, `net route-remove`
- `firewall status`, `firewall rule-add`, `firewall rule-update`, `firewall rule-delete`, `firewall flush`, `firewall policy`
- `storage devices`, `storage mounts`, `storage mount`, `storage unmount`
- `accounts users`, `accounts groups`, `accounts user-create`, `accounts user-delete`, `accounts user-lock`
- `home info`

## Views

The app source lives under `apps/<name>/` (`manifest.toml.in`, `wit/` and
`frontend/`); the demo builder packages each `frontend/dist` module with the
shared emulator service. Every bundle carries a `panel.view` module:
- `apps/fs/frontend/` — Files plus a hidden `panel.view` called Editor, both from the
  same module. Files has an editable address bar, tree navigation, a formatted
  read-only preview (highlight.js for code, marked for Markdown, images via a
  blob preview), **New** (blank file, folder or symlink), symlink navigation,
  rename, upload, download, trash and restore, plus delete. Uploads and downloads
  go through a visible transfer queue with progress, conflicts, retry and cancel;
  dragging files onto the view (or a folder row) queues uploads to that folder.
  MIME-aware folder and file icons carry the extension in the badge (image,
  video, audio, database, archive, code, config...). On narrow screens
  the preview opens full-screen only after selecting a file, with a back
  button. **Open in Editor** opens the file in the panel-level Editor: it
  reuses the docked tab when the Editor is docked, and a floating window
  otherwise. The editor is CodeMirror 6: syntax highlighting, autocomplete,
  line numbers, lint diagnostics, internal file tabs (text and image tabs, with
  image previews), its own file tree (inline or mobile drawer), line/column and
  selection counters, EOL, detected language, and indentation size/tab
  character. Shortcuts include
  `Shift+Tab` to outdent, `Ctrl+/` to toggle comments and `Ctrl+Space` to open
  completion. Closing a dirty tab or the Editor opens a real `<dialog>` with
  Save all / Discard / Cancel; no `confirm()` is used. No inline editing or
  in-frame floating editor remains.
- `apps/terminal/frontend/` — direct typing in the terminal surface (an
  invisible `contenteditable` capture only opens the soft keyboard, avoiding
  form autofill), help/exit, history, INT/TERM/Clear, reconnect and resize. On
  phones it adds a scrollable key bar with a keyboard show/hide toggle, sticky
  Ctrl/Alt, Esc, Tab, arrows, Home/End, PgUp/PgDn, ^C/^D/^L/^Z, symbols, Enter
  and A−/A+. Double-tap sends Tab, pinch changes the font size, and long-press
  plus drag sends arrow keys; a long press without movement keeps native text
  selection. `interactive-widget=resizes-content` keeps the bar above the soft
  keyboard.
- `apps/proc/frontend/` — flat/tree process table, interval refresh with a manual `0s`
  mode, allowlisted signals and
  detached spawn.
- `apps/net/frontend/` — live interfaces and routes over SSE, addresses/counters, up/down, route add/remove and an interval selector with a manual `0s` mode.
- `apps/firewall/frontend/` — the managed nftables table as a sub-tab under Network: chain policies, allow/deny rules, custom rules, add/edit/delete, flush and policy confirmation.
- `apps/storage/frontend/` — mount usage bars, unmount and mount.
- `apps/accounts/frontend/` — users and groups with create, delete and lock/unlock.
- `apps/home/frontend/dist/home.js` — Host Info cards (system, CPU, memory,
  disks and network). The demo bundle answers from the shared emulator's host
  fixture, so the values are simulated and move over time; `apps/home/build.sh`
  packages the real `/proc` reader.

The panel mounts them like any other slot; the modules only see their own app's
routes.

## State

The fixtures are deterministic. Files, accounts, network and storage are durable
and **shared**: every stateful bundle reads and writes one JSON snapshot at
`state/shared/emulator/host.json` (`0600`, atomic write, cross-process locked).
A mountpoint created by Storage appears in Files, a new account's home is created
in Files, and Home reads the same network/mount state, so the apps describe one
host. It
survives a panel/service restart. Delete `host.json` to reseed the whole demo.

Files also follows Storage: mounting a device overlays its files at the
mountpoint, unmounting hides them, and changes made while mounted are kept on
the device and restored on the next mount.

The process table, terminal sessions and Home's metrics are runtime state and
reset on restart. Removing an app from the composition degrades only that view.

The emulator is not engine code. It lives outside the wasm `apps/` workspace, is
unsigned, declares `socket` for the six service apps and `host` for Home, and
never receives the host root.

## Not in the simulator

Firewall rules, interface create/rename/delete, format/partition/swap, password
and group mutations, and a real PTY with ANSI emulation are **real-service
scope**. The simulator stays focused on the demo UI and the protocol.
