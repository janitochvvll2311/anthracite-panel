# Processes

Processes is a native `/proc` service plus the sandboxed panel view. The demo
instance uses the shared scripted fixture from `demo/emulator`; this directory
builds the real app.

- `backend/` reads `/proc`, computes CPU deltas between snapshots, reports
  resident memory and maps process states to the WIT enum. `signal` requires the
  `pid + start-ticks` pair, so a recycled pid is never hit; `spawn` is limited to
  the `sleep`, `echo` and `true` allowlist and detaches the child with `setsid`.
- `frontend/` is the existing process view: paginated table or tree, CPU and
  memory, allowlisted signal actions and the detached-spawn form. An interval
  selector drives the refresh; `Manual (0s)` stops the timer and leaves Refresh
  as the only update.

## Build

```bash
apps/proc/build.sh
# writes apps/proc/dist/proc.anth
```

The script builds and type-checks the frontend, builds `proc-service`, hashes the
binary and module, and packages `manifest.toml`, `wit/proc.wit`, `web/proc.js`
and `bin/proc-service`.

## Install and approve

The real service declares `class = "admin"`:

```bash
ANTH_SOCKET=/path/to/engine.sock
./target/debug/anth --socket "$ANTH_SOCKET" app upgrade org.anthracite.proc \
    apps/proc/dist/proc.anth
./target/debug/anth --socket "$ANTH_SOCKET" app approve org.anthracite.proc \
    --accept-write --accept-admin
```

It replaces the emulator bundle under the same app id. Restart the panel after
the upgrade so the new service starts.
