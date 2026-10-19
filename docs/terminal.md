# Terminal

The daemon can run an interactive program on a pseudo-terminal and stream it over
the socket protocol. This document covers enabling it, what it exposes, and what
it does not protect you from.

For the wire format, see [socket-protocol.md](socket-protocol.md#terminal).

## Warning

A terminal is remote code execution. Anyone who can open a session gets a shell
as the user the daemon runs as.

The panel and the daemon have **no authentication**. The terminal adds a token of
its own, but that token is not a user system: it is one shared secret for one
operator. Do not expose the socket to a network you do not control. If you need
the terminal reachable from a browser, put it behind a reverse proxy that
terminates TLS and authenticates users, and keep the query string out of the
access logs.

## Enabling it

The terminal is off unless a token is configured:

```toml
socket_path = "/var/tmp/anth.sock"

[terminal]
shell = "/bin/sh"        # program to spawn per session
token = "a-long-random"  # empty means disabled
max_sessions = 4         # concurrent sessions across the daemon
```

| Setting | Default | Meaning |
| --- | --- | --- |
| `shell` | `/bin/sh` | Program spawned for each session. |
| `token` | `""` | Empty disables the feature. |
| `max_sessions` | `4` | Concurrent sessions; further opens are refused, not queued. |

Generate a token with `head -c 32 /dev/urandom | base64`.

## Behaviour worth knowing

- **Sessions belong to a connection.** When the client disconnects, its sessions
  are killed. This includes a client that crashes or drops off the network.
- **The process group is killed**, not just the shell, so a background job
  started in the session does not survive it.
- **The child gets a minimal environment**: `TERM`, `PATH` and `HOME` only. The
  daemon's own environment is not inherited, so credentials in it do not leak
  into a shell.
- **Size is enforced.** Sizes outside 1..1000 columns/rows are refused rather
  than clamped, so a client cannot make the daemon allocate an absurd buffer.
- **The line cap is 1 MiB**, and one terminal frame is capped at 64 KiB.
- **Input, resize and close are silent on success.** Only failures produce a
  frame, so a client cannot correlate acknowledgements with requests. See
  [socket-protocol.md](socket-protocol.md#silent-commands).

## What is not implemented yet

- No HTTP or WebSocket route. The terminal speaks the socket protocol only;
  the browser layer is a separate piece of work.
- No idle timeout. A session with a connected client stays open until the
  client closes it or the daemon stops.
- No `enabled` flag separate from the token: an empty token means disabled, and
  there is no way to have a token configured but the feature off.

## Testing it by hand

With a daemon running and a token set, a client is one socket and two frames:

```bash
printf '%s\n' \
  '{"type":"TerminalOpen","data":{"size":{"cols":80,"rows":24}}}' \
  | socat - UNIX-CONNECT:/var/tmp/anth.sock
```

The reply carries the session `id`. `TerminalInput` then takes base64 bytes, and
output arrives as `TerminalOutput` frames without being asked for. Do not wait for
a reply to input: success is silent and only errors answer.
