# Demo instance

The v0.6.0 demo runs the deterministic host emulator as six service apps:
`org.anthracite.fs`, `terminal`, `proc`, `net`, `storage` and `accounts`. Each
bundle carries the future real app's WIT package and id, and all six share one
native binary.

The engine is a debug build here, so `[services] allow-third-party` defaults on
and the unsigned bundles are accepted. A release build keeps both closed.

The emulator claims the future first-party ids and WIT packages (`org.anthracite.fs`,
`anthracite:fs`, …). In a release those bundles are `system`-signed; in the demo
they are third-party and run under the explicit developer switch.

## Build the bundles

```bash
# Only needed after editing the Files editor source; the generated module is
# committed and already bundles ProseMirror.
demo/emulator/build-web.sh

cargo build --manifest-path demo/emulator/Cargo.toml --release
cargo run --manifest-path demo/emulator/Cargo.toml --release --bin bundle-emulator -- \
    --binary target/release/host-emulator --out demo/dist
```

`demo/dist/{fs,terminal,proc,net,storage,accounts}.anth` are the installable zips.

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
for app in fs terminal proc net storage accounts; do
    $ANTH app install demo/dist/$app.anth
    $ANTH app approve org.anthracite.$app --accept-write --accept-admin
done
$ANTH compose --add org.anthracite.fs --add org.anthracite.terminal \
    --add org.anthracite.proc --add org.anthracite.net \
    --add org.anthracite.storage --add org.anthracite.accounts --now
```

A v0.5 engine recomposes on the next restart; restart the panel after `compose`.

Then, from the panel session (or the CLI), the app routes answer:

```bash
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.fs/files?path=/
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.proc/processes
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.net/interfaces
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.storage/mounts
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.accounts/users
```

CLI verbs:

- `fs files`, `fs file-stat`, `fs file-read`, `fs trash`, `fs rename`, `fs restore`
- `terminal sessions`, `terminal session-open`, `terminal session-read`, `terminal session-close`
- `proc processes`, `proc process-signal`, `proc process-spawn`
- `net interfaces`, `net routes`, `net interface-up`, `net route-add`, `net route-remove`
- `storage devices`, `storage mounts`, `storage mount`, `storage unmount`
- `accounts users`, `accounts groups`, `accounts user-create`, `accounts user-delete`, `accounts user-lock`

## Views

Every bundle carries a `panel.view` module:

- `web/fs.js` — Files plus a second `panel.view` called Editor, both from the
  same module. Files has an editable address bar, tree navigation, preview,
  **New** (blank file, folder or symlink), symlink navigation, rename, upload,
  download, trash and restore, plus delete. Edit opens a ProseMirror code editor
  with line-based syntax highlighting; **Pop out** opens the same file in the
  panel-level Editor view, which then uses the normal panel Float/Dock/Max/Min
  controls and keeps its text across moves. There is no in-frame floating
  editor: if the panel cannot open the view, the inline editor stays open.
- `web/terminal.js` — session with help/exit, history, INT/TERM/Clear, reconnect
  and resize.
- `web/proc.js` — flat/tree process table, live refresh, allowlisted signals and
  detached spawn.
- `web/net.js` — interfaces with up/down, addresses/counters and route add/remove.
- `web/storage.js` — mount usage bars, unmount and mount.
- `web/accounts.js` — users and groups with create, delete and lock/unlock.

The panel mounts them like any other slot; the modules only see their own app's
routes.

## State

The fixtures are deterministic. Files, accounts, network and storage are durable:
each of those bundles writes a plain JSON snapshot to
`state/data/<app-id>/state.json` (`0600`, atomic write) through the approved
`{data}` argv. They survive a panel/service restart. Delete an app’s
`state.json` to reseed that domain.

The process table and terminal sessions are runtime state and reset on restart.
Removing an app from the composition degrades only that view.

The emulator is not engine code. It lives outside the wasm `apps/` workspace, is
unsigned, class `socket`, and never receives the host root.

## Not in the simulator

Firewall rules, interface create/rename/delete, format/partition/swap, password
and group mutations, and a real PTY with ANSI emulation are **real-service
scope**. The simulator stays focused on the demo UI and the protocol.
