# Web terminal: design proposal

Status: partially implemented on `feat/web-terminal`. The daemon already owns the
PTY and carries it over the line protocol; `GET /api/terminal` bridges a browser
WebSocket to one daemon connection, and the frontend uses an xterm-backed Lit
component. Authentication, idle timeouts and automatic reconnect are still
pending.

The implemented shape is **B**, not the branch recommendation below: the daemon
owns the PTY and the panel only upgrades the browser socket and translates
frames. The existing line protocol already carries base64 bytes, so no
`anth-proto` framing change was required.

## The problem this has to solve before the rest

A web terminal is remote code execution behind an HTTP route. `anth-panel` today
has **no authentication at all**, and `run`/`api` bind `0.0.0.0:8080` by default
(`DEFAULT_ADDR` in `main.rs`). Every route that exists now is read-only or an
echo, so the exposure is academic. Adding a PTY route turns it into a shell for
anyone who can reach the port. That is the first design decision, not an
afterthought.

Two things follow, and both are in scope:

1. The panel keeps no authentication of its own. It assumes a reverse proxy or a
   private network is the boundary, and it must be explicit about that.
2. The terminal gets a **separate, opt-in token gate** that no other route
   requires, so existing deployments do not break and a terminal is never one
   config mistake away from being open.

The alternative — "add auth to the whole panel" — is a bigger product decision
(sessions, login UI, password storage) and is not what was asked for. It stays
out of scope, but the terminal must not be the reason that decision gets forced.

## Options considered

**A. The panel spawns the PTY directly.** WebSocket handler calls `openpty`,
forks a shell, pumps bytes. Simplest, fewest moving parts.

**B. The daemon owns the PTY; the panel proxies over the Unix socket.** The
daemon is already the privileged component that talks to the host. The panel
stays a pure HTTP/transport layer, exactly as it is for `host-info`.

Fold in a constraint from the protocol doc: `anth-proto` is the published
contract that third-party clients implement against, and it is newline-delimited
JSON. Interactive terminal I/O is binary, high-volume and latency-sensitive.
Putting it through `LinesCodec` would mean base64-in-JSON on every keystroke and
every chunk of output, and would change the framing guarantees the doc makes.

## Recommendation: A now, B later, and say why

**A** for this branch. Reasons:

- The panel and the daemon are already one process in `run`, and the Docker image
  ships them together. Splitting the PTY into the daemon buys isolation that does
  not exist yet: same binary, same uid, same process.
- It keeps `anth-proto` clean. A binary streaming channel inside a
  newline-JSON protocol is a protocol version bump, not a feature.
- It is testable end to end in this repo, which B is not without inventing a
  second transport first.

**B** is the right long-term shape if the daemon ever runs as a different user or
in a different process from the panel, because then the privilege boundary is
real. Doing it now would mean designing a framing change to `anth-proto` that
nothing currently needs. I would rather land A behind a narrow interface so B is
a swap of the backend, not a rewrite.

To keep that open, the PTY sits behind a trait (`Session`), and the WebSocket
handler only knows that trait. Swapping in a socket-backed implementation later
does not touch the route, the protocol, or the frontend.

## Architecture

```
frontend/                              backend/anth-panel/src/
  src/components/anth-terminal.ts        api.rs          route + WebSocketUpgrade
  src/lib/terminal/transport.ts          ws.rs           generic JSON/WebSocket
  src/lib/terminal/protocol.ts           terminal/web.rs browser frames <-> daemon
  src/lib/terminal/websocket-transport.ts terminal/session.rs PTY spawn/read/write
  src/lib/ws/json-websocket.ts           terminal/registry.rs lifecycle + cap
                                         terminal/auth.rs token helper (not wired)
```

### Layers and who owns what

| Layer | Owns | Does not know about |
| --- | --- | --- |
| `api.rs` | route, `WebSocketUpgrade`, query validation | PTY details, JSON frames |
| `ws.rs` | text-frame JSON, size cap, control frames | terminal semantics |
| `terminal/web.rs` | message mapping, daemon connection, close/exit | HTTP upgrade |
| `terminal/session.rs` | spawn, read, write, resize, kill | HTTP, JSON framing |
| `terminal/registry.rs` | lifecycle, session cap, output channels | transports |
| `frontend/.../json-websocket.ts` | WebSocket state, JSON encode/decode | terminal semantics |
| `frontend/.../websocket-transport.ts` | base64 frames, heartbeat, close/exit | xterm |
| `frontend/.../anth-terminal.ts` | xterm rendering, keys, resize, local history | route or wire format |

This mirrors what is already there: `poll.rs` owns *when*, `sse.rs` owns
*framing*, `stream.rs` owns *repetition*. Same idea applied to a duplex channel.

### Wire protocol (panel <-> browser)

Text frames carrying JSON, with terminal bytes base64-encoded. Not binary frames:
one code path for control and data keeps the client simple, and the size is
irrelevant next to the round trip. The encoding is stated once, in
`terminal/web.rs`, and the frontend has a single decoder in
`lib/terminal/protocol.ts`.

```jsonc
// client -> server
{ "type": "input",  "data": "<base64>" }
{ "type": "resize", "cols": 120, "rows": 40 }
{ "type": "ping" }

// server -> client
{ "type": "ready",  "shell": "/bin/sh", "cols": 80, "rows": 24 }
{ "type": "output", "data": "<base64>" }
{ "type": "exit",   "code": 0 }
{ "type": "error",  "detail": "pty unavailable" }
{ "type": "pong" }
```

Decisions worth arguing about:

- **`ready` first, always.** The client needs the real size before it paints, and
  a failure must arrive as a message rather than a close code the browser cannot
  read.
- **`exit` then close.** Distinguishes "the shell finished" from "the network
  died", which the UI shows differently.
- **Cap inbound messages** (e.g. 64 KiB per frame, rejects oversized) so a client
  cannot make the server allocate without bound.
- **No resize echo from the server.** The client is the source of truth for size;
  echoing invites loops.

### Auth

> Not implemented yet. The current route ignores `terminal.token` and accepts
> any browser that can reach the port. The rest of this section is the intended
> design.

`terminal.token` in the config file. Empty or absent means the route is
**disabled** and answers `404`, not `403`: a disabled feature should not
advertise itself. When set, the token arrives as a query parameter on the
upgrade (`?token=...`), because browsers cannot set headers on a WebSocket
handshake. Consequences, all accepted:

- The token can land in proxy access logs. Documented, and the proxy config in
  the README is expected to redact the query string.
- Comparison is constant-time.
- No token in the frontend bundle. The user pastes it once; it lives in
  `sessionStorage`, not `localStorage`.

This is a lock, not a user system. It is proportionate to "one operator, one
token, private network" and I would say so in the docs rather than imply more.

### Session lifecycle

- One PTY per WebSocket. Closing the socket kills the process group; a client
  that vanishes must not leave a shell running.
- A global cap (config `terminal.max_sessions`, default 4). This is the
  lesson from the SSE review, applied before it bites: unbounded per-client
  resources are a bug waiting for a retry loop.
- An idle timeout is still pending.
- Env is scrubbed: the shell inherits a minimal environment (`TERM`, `PATH`,
  `HOME`, `LANG`, `LC_ALL`) rather than the panel's.
- `SIGWINCH` on resize, so `vim`/`htop`/`btop` behave.
- Child exit is observed, not polled: `exit` carries the real code.

### Frontend

- `@xterm/xterm` 6.0.0 plus `@xterm/addon-fit`. Both are small and are the
  standard; a hand-rolled emulator is not worth writing.
- `src/components/anth-terminal.ts` is a Lit element like the rest; it owns the
  xterm instance, the window chrome, resize handling and a local Up/Down
  command history on the normal buffer. Full-screen programs keep their arrows.
- `src/lib/terminal/websocket-transport.ts` implements the transport interface
  the component consumes; `src/lib/ws/json-websocket.ts` is the generic,
  terminal-agnostic JSON WebSocket layer.
- Reconnect with exponential backoff is **not implemented yet**. When it lands,
  it should apply only to network drops, not after `exit`.
- xterm is loaded lazily (`import()`), so the panel's initial bundle does not pay
  for it.

## Config

```toml
socket_path = "/var/tmp/anth.sock"

[terminal]
shell = "/bin/sh"
token = ""               # reserved for the auth layer; not enforced yet
max_sessions = 4
```

`Config` uses `#[serde(default, deny_unknown_fields)]`, so a new optional table
keeps every existing config valid. An `enabled` flag and idle timeout remain
proposals; only `shell`, `token` and `max_sessions` exist today.

## Testing

The project's convention is real code paths, no mocks. For a PTY that means:

- Spawn a real `/bin/sh` (present in the test image) and assert on bytes:
  write `echo hi\n`, read until `hi` appears. Guard with `#[cfg(unix)]`.
- Real HTTP upgrade against a bound listener, then a real WebSocket client
  (`tokio-tungstenite`, already in the tree via `axum/ws`) — not a fake socket.
- Cases that must fail loudly: invalid query, daemon unavailable, session cap
  reached, oversized frame, malformed message, resize with absurd dimensions.
  Each asserts a specific status or message, not just "an error". Auth rejection
  tests land with the auth layer.
- One test that closes the socket and then checks the child is gone, because
  leaking shells across a reconnect is the failure this design exists to avoid.

## Implementation order used

1. `anth-proto` terminal variants and base64 helpers, with wire tests.
2. `terminal/session.rs` behind the `Session` trait, tested against a real
   `/bin/sh`.
3. `ws.rs` generic JSON/WebSocket wrapper and `terminal/web.rs` message mapping.
4. The WebSocket bridge: open, input, output, resize, exit, error and cleanup.
5. Frontend: `json-websocket.ts`, `protocol.ts`, `websocket-transport.ts`,
   `anth-terminal.ts` and lazy xterm loading.
6. Docs: `docs/terminal.md`, `docs/api-endpoints.md`, README and `AGENTS.md`.

Authentication, idle timeout and automatic reconnect are follow-ups.

## Open questions for you

1. **Shell**: `/bin/sh` fixed, or configurable? Busybox alpine has no bash, and a
   configurable shell is a smaller step from "run a shell" to "run anything".
2. **Working directory**: start at `/`, `$HOME`, or the daemon's cwd?
3. **Should the token gate apply to the whole panel** in a follow-up? I have kept
   it terminal-only to avoid forcing a bigger decision now, but you may prefer
   one boundary for everything.
4. **Multiple tabs / sessions per user**: the cap is global today. Per-IP would
   need care behind a proxy and is probably premature.
