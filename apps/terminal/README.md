# Terminal

Terminal is a native PTY service plus the sandboxed panel view. The demo
instance still uses the shared scripted fixture from `demo/emulator`; this
directory builds the real app.

- `backend/` starts the requested shell in a pseudo-terminal, follows its output
  as a live stream (`subscription.follow`), resizes the PTY, forwards allowlisted
  signals to the process group and closes the session by killing that group. The
  old `read` call stays for the CLI and older clients.
- `frontend/` is the panel module. It renders the PTY stream with xterm.js, so
  ANSI colors, cursor movement, alternate screens and clear sequences work. When
  the panel relays WebSockets it uses `/socket` for both output and input, so a
  keystroke is not an HTTP call; otherwise it falls back to `/stream` plus
  `/write`. Direct typing goes through an invisible `contenteditable` capture
  (which avoids form autofill) and the mobile key bar adds sticky Ctrl/Alt, special
  keys, symbols, Enter, font controls and touch gestures.

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
