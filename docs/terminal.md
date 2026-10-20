# Terminal

The daemon can run an interactive program on a pseudo-terminal and stream it over
the socket protocol. This document covers enabling it, what it exposes, and what
it does not protect you from.

For the wire format, see [socket-protocol.md](socket-protocol.md#terminal).

## Warning

A terminal is remote code execution. Anyone who can open a session gets a shell
as the user the daemon runs as.

The **daemon socket has no authentication**. Anyone who can open it gets a
shell. The HTTP panel sits behind the session layer described in
[api-endpoints.md](api-endpoints.md#authentication): with `[auth] enabled` the
WebSocket upgrade needs the `anth_access` cookie, so a browser that has not
signed in cannot reach a PTY. That covers the browser boundary only, not the
socket.

Do not expose the socket to a network you do not control, and do not enable the
panel over plain HTTP on an untrusted network: the session cookie travels on
every request. Set `[auth] cookie_secure = true` once TLS terminates in front of
the panel. `terminal.token` is still not enforced; it is reserved for a
per-session capability, which the panel session does not replace.

## Enabling it

The `[terminal]` table selects the program and the session cap:

```toml
socket_path = "/var/tmp/anth.sock"

[terminal]
shell = "auto"           # $SHELL / login shell, then /bin/sh
token = "a-long-random"  # reserved; not enforced yet
max_sessions = 4         # concurrent sessions across the daemon
```

| Setting | Default | Meaning |
| --- | --- | --- |
| `shell` | `"auto"` | Program spawned for each session. `"auto"` uses `$SHELL`, then the account's login shell from `/etc/passwd`, then `/bin/sh`. |
| `token` | `""` | Reserved for the authentication layer; currently ignored. |
| `max_sessions` | `4` | Concurrent sessions; further opens are refused, not queued. |

An existing config that explicitly says `shell = "/bin/sh"` keeps that choice;
change it to `"auto"` or `/bin/bash` if that deployment should get shell-native
completion.

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
- **The browser terminal chooses between shell-native and local keys.** With
  `keyboard-mode = "auto"` (the default), a shell with line editing (`bash`,
  `ash`, `zsh`, etc.) handles Up/Down and Tab natively; a shell without it
  (`dash`, `/bin/sh` on Debian) falls back to the component's local history and
  prefix completion. The default `shell = "auto"` resolves `$SHELL`, then the
  account's login shell from `/etc/passwd`, then `/bin/sh`; set
  `shell = "/bin/bash"` to force bash. Full-screen programs such as `btop`,
  `vim` and `htop` always keep their arrows and Tab. The shell environment
  forces `C.UTF-8`, which those tools need to start.

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
