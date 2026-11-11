# Home

Home is the host overview view. It replaces Hello in the demo composition, while
`apps/hello` stays in the tree as the minimal developer example.

The view is deliberately split:

- `frontend/` is the sandboxed Bun/TypeScript panel module. It follows its own
  `/api/apps/org.anthracite.home/watch` SSE route and keeps `/info` as the
  refresh/fallback path.
- `backend/` is a native Rust service that reads host facts (`/proc`, `df`, `ip`)
  and answers `anthracite:home/host@1.0.0#info` over the private service socket.
  `watch` returns a `stream<host-info>`; the engine frames each snapshot as one
  `text/event-stream` event. It declares `class = "host"`, so approval needs
  `--accept-write --accept-admin --accept-host` (the engine requires every level
  up to the declared risk).

The demo does not use this backend. `demo/emulator` answers the same contract
with deterministic fixtures, so a demo instance never exposes the machine's real
hostname, addresses or disks. Screenshots and recordings use that bundle.

## Build

```bash
apps/home/build.sh           # real host reader, writes apps/home/dist/home.anth
```

The script builds the Bun/TypeScript frontend, runs `tsc --noEmit`, builds the
backend, hashes both, and packages `manifest.toml`, `wit/home.wit`,
`web/home.js` and `bin/home-service`.

The demo bundle is built by `demo/emulator` from the same manifest, WIT and
frontend; only the packaged binary changes. The frontend values move over time
(CPU, load, memory, disk usage, network counters and uptime).

## Install and compose

```bash
ANTH_SOCKET=demo/state/engine.sock
./target/debug/anth --socket "$ANTH_SOCKET" app install apps/home/dist/home.anth
./target/debug/anth --socket "$ANTH_SOCKET" app approve org.anthracite.home \
    --accept-write --accept-admin --accept-host
./target/debug/anth --socket "$ANTH_SOCKET" compose \
    --add org.anthracite.home --remove org.anthracite.hello --now
```

Hello remains installed if it was there before; only the composition changes.
It remains in `apps/hello` as an example.
