# Terminal

The daemon can run an interactive program on a pseudo-terminal and stream it over
the socket protocol. This document covers enabling it, what it exposes, and what
it does not protect you from.

For the wire format, see [socket-protocol.md](socket-protocol.md#terminal).

## Warning

A terminal is remote code execution. Anyone who can open a session gets a shell
as the user the daemon runs as.

The panel and the daemon have **no authentication**. The terminal configuration
has a `token` setting, but in the current development build it is **not
enforced** by either the daemon or the WebSocket route. Do not expose the socket
or the HTTP port to a network you do not control. The token is reserved for the
authentication layer that will be added at the browser boundary.

## Enabling it

The `[terminal]` table selects the program and the session cap:

```toml
socket_path = "/var/tmp/anth.sock"

[terminal]
shell = "/bin/sh"        # program to spawn per session
token = "a-long-random"  # reserved; not enforced yet
max_sessions = 4         # concurrent sessions across the daemon
```

| Setting | Default | Meaning |
| --- | --- | --- |
| `shell` | `/bin/sh` | Program spawned for each session. |
| `token` | `""` | Reserved for the authentication layer; currently ignored. |
| `max_sessions` | `4` | Concurrent sessions; further opens are refused, not queued. |

The browser reaches the same sessions through
[`GET /api/terminal`](api-endpoints.md#get-apiterminal), which upgrades to a
WebSocket and bridges it to the daemon. The CLI reaches them through
[`anth-panel terminal`](cli.md).

## Behaviour worth knowing

- **Sessions belong to a connection.** When the client disconnects, its sessions
  are killed. This includes a client that crashes or drops off the network.
- **The process group is killed**, not just the shell, so a background job
  started in the session does not survive it.
- **The child gets a minimal environment**: `TERM`, `PATH`, `HOME`, `LANG` and
  `LC_ALL` only. The locale is forced to `C.UTF-8` so tools such as `btop` can
  start; the daemon's own environment is not inherited, so credentials in it do
  not leak into a shell.
- **Size is enforced.** Sizes outside 1..1000 columns/rows are refused rather
  than clamped, so a client cannot make the daemon allocate an absurd buffer.
- **The line cap is 1 MiB**, and one terminal frame is capped at 64 KiB.
- **Input, resize and close are silent on success.** Only failures produce a
  frame, so a client cannot correlate acknowledgements with requests. See
  [socket-protocol.md](socket-protocol.md#silent-commands).
- **The browser terminal keeps a local command history.** Up/Down walk the
  commands typed in the component while the normal screen buffer is active.
  Tab completes the current line from that history and cycles through matches.
  Full-screen programs such as `btop`, `vim` and `htop` use the alternate
  buffer, where arrow keys and Tab pass through to the program. The shell
  environment forces `C.UTF-8`, which those tools need to start.

## What is not implemented yet

- **Authentication.** The token is stored but not checked. The route is open to
  anyone who can reach the HTTP port.
- No idle timeout. A session with a connected client stays open until the
  client closes it or the daemon stops.
- No `enabled` flag separate from the token: there is no way to keep the
  terminal off while a token is configured.

## Testing it by hand

With a daemon running, the shortest client is the built-in CLI command:

```bash
anth-panel terminal
```

For the raw wire shape, a client is one socket and two frames:

```bash
printf '%s\n' \
  '{"type":"TerminalOpen","data":{"size":{"cols":80,"rows":24}}}' \
  | socat - UNIX-CONNECT:/var/tmp/anth.sock
```

The reply carries the session `id`. `TerminalInput` then takes base64 bytes, and
output arrives as `TerminalOutput` frames without being asked for. Do not wait for
a reply to input: success is silent and only errors answer.
