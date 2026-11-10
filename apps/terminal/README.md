# Terminal

Terminal is a native PTY service plus the sandboxed panel view. The demo
instance still uses the shared scripted fixture from `demo/emulator`; this
directory builds the real app.

- `backend/` starts the requested shell in a pseudo-terminal, streams its output
  in chunks, resizes the PTY, forwards allowlisted signals to the process group
  and closes the session by killing that group.
- `frontend/` is the panel module. It renders output in a plain surface (ANSI
  control sequences are stripped, since this is not an xterm emulator), keeps a
  local line draft and implements the mobile key bar, sticky Ctrl/Alt and touch
  gestures.

## Build

```bash
apps/terminal/build.sh
# writes apps/terminal/dist/terminal.anth
```

The script builds and type-checks the frontend, builds `terminal-service`, hashes
the binary and module, and packages `manifest.toml`, `wit/terminal.wit`,
`web/terminal.js` and `bin/terminal-service`.

## Install and approve

The native service declares `class = "exec"`, so approval needs the full risk
ladder:

```bash
ANTH_SOCKET=/path/to/engine.sock
./target/debug/anth --socket "$ANTH_SOCKET" app install apps/terminal/dist/terminal.anth
./target/debug/anth --socket "$ANTH_SOCKET" app approve org.anthracite.terminal \
    --accept-write --accept-admin --accept-host
```

It replaces the emulator bundle under the same app id. Reconnect the Terminal
view after the panel restarts.
