# Home

Home is the host overview view. It replaces Hello in the demo composition, while
`apps/hello` stays in the tree as the minimal developer example.

The view is deliberately split:

- `frontend/` is the sandboxed Bun/TypeScript panel module. It only calls its
  own `/api/apps/org.anthracite.home/info` route.
- `backend/` is a native Rust service that reads host facts (`/proc`, `df`, `ip`)
  and answers `anthracite:home/host@1.0.0#info` over the private service socket.
  It declares `class = "host"`, so approval needs `--accept-write --accept-admin
  --accept-host` (the engine requires every level up to the declared risk).
  `home-demo` answers the same contract with fixtures and is what
  `--demo` packages.

## Build

```bash
apps/home/build.sh           # real host reader, writes apps/home/dist/home.anth
apps/home/build.sh --demo    # deterministic fixture, same bundle layout
```

The script builds the Bun/TypeScript frontend, runs `tsc --noEmit`, builds the
backend, hashes both, and packages `manifest.toml`, `wit/home.wit`,
`web/home.js` and `bin/home-service`.

`--demo` swaps `bin/home-service` for the `home-demo` binary: same WIT contract
and same frontend, simulated host facts that move over time (CPU, load, memory,
disk usage, network counters and uptime). The demo instance uses it so
screenshots and recordings never expose the machine's real hostname, addresses
or disks.

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
