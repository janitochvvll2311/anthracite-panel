# Demo instance

The v0.6.0 demo runs the deterministic host emulator as two service apps:
`org.anthracite.fs` and `org.anthracite.terminal`. Each bundle carries the future
real app's WIT package and id, and both share one native binary.

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

`demo/dist/fs.anth` and `demo/dist/terminal.anth` are the installable zips.

## Run

From the repository root:

```bash
cargo run --manifest-path panel/backend/Cargo.toml -- \
    --config demo/config.toml --listen 127.0.0.1:5173
```

The first boot writes `demo/state/` and creates the operator account. In another
terminal, using the CLI:

```bash
cargo run --manifest-path sdk/rust/Cargo.toml -p anth-cli -- \
    --socket demo/state/engine.sock app install demo/dist/fs.anth
cargo run --manifest-path sdk/rust/Cargo.toml -p anth-cli -- \
    --socket demo/state/engine.sock app install demo/dist/terminal.anth
cargo run --manifest-path sdk/rust/Cargo.toml -p anth-cli -- \
    --socket demo/state/engine.sock app approve org.anthracite.fs \
    --accept-write --accept-admin
cargo run --manifest-path sdk/rust/Cargo.toml -p anth-cli -- \
    --socket demo/state/engine.sock app approve org.anthracite.terminal \
    --accept-write --accept-admin
cargo run --manifest-path sdk/rust/Cargo.toml -p anth-cli -- \
    --socket demo/state/engine.sock compose \
    --add org.anthracite.fs --add org.anthracite.terminal --now
```

A v0.5 engine recomposes on the next restart; restart the panel after `compose`.

Then, from the panel session (or the CLI), the app routes answer:

```bash
curl --unix-socket demo/state/engine.sock http://localhost/api/apps/org.anthracite.fs/files?path=/
curl --unix-socket demo/state/engine.sock \
    -X POST http://localhost/api/apps/org.anthracite.terminal/open \
    -H 'content-type: application/json' \
    -d '{"shell":"/bin/sh","cols":80,"rows":24}'
```

The CLI verbs are `fs files`, `fs file-stat`, `fs file-read`, `terminal sessions`,
`terminal session-open`, `terminal session-read` and `terminal session-close`.

## What is deterministic

The tree, sizes, timestamps, session ids and command output are fixed; writes and
sessions live in memory and reset when the service restarts. Removing either app
from the composition degrades only that view.

The emulator is not engine code. It lives outside the wasm `apps/` workspace, is
unsigned, class `socket`, and never receives the host root.
