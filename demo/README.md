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

- `fs files`, `fs file-stat`, `fs file-read`
- `terminal sessions`, `terminal session-open`, `terminal session-read`, `terminal session-close`
- `proc processes`, `proc process-signal`
- `net interfaces`, `net routes`
- `storage devices`, `storage mounts`
- `accounts users`, `accounts groups`

## Views

Every bundle carries a `panel.view` module:

- `web/fs.js` — tree explorer with preview, new folder and move-to-trash.
- `web/terminal.js` — opens a session, polls `read` and echoes commands.
- `web/proc.js` — process table with CPU/memory and an allowlisted signal action.
- `web/net.js` — interfaces with addresses/counters and the routing table.
- `web/storage.js` — mount usage bars and block devices.
- `web/accounts.js` — users and groups with system/locked badges.

The panel mounts them like any other slot; the modules only see their own app's
routes.

## What is deterministic

The tree, sizes, timestamps, session ids, process table, interfaces, mounts and
accounts are fixed; writes, sessions and signals live in memory and reset when
the service restarts. Removing an app from the composition degrades only that
view.

The emulator is not engine code. It lives outside the wasm `apps/` workspace, is
unsigned, class `socket`, and never receives the host root.
