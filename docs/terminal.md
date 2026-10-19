# Terminal

The daemon can run an interactive program on a pseudo-terminal and stream it over
the socket protocol. This document covers enabling it, what it exposes, and what
it does not protect you from.

For the wire format, see [socket-protocol.md](socket-protocol.md#terminal).

## Warning

A terminal is remote code execution. Anyone who can open a session gets a shell
as the configured terminal user, which defaults to `anth`.

The panel and the daemon have **no authentication**. The terminal configuration
has a `token` setting, but in the current development build it is **not
enforced** by either the daemon or the WebSocket route. Do not expose the socket
or the HTTP port to a network you do not control. The token is reserved for the
authentication layer that will be added at the browser boundary.

## Enabling it

The `[terminal]` table selects the program, the account and the session cap:

```toml
socket_path = "/var/tmp/anth.sock"

[terminal]
shell = "auto"           # $SHELL / login shell, then /bin/sh
user = "anth"            # account the shell runs as; "auto" = daemon user
token = "a-long-random"  # reserved; not enforced yet
max_sessions = 4         # concurrent sessions across the daemon
```

| Setting | Default | Meaning |
| --- | --- | --- |
| `shell` | `"auto"` | Program spawned for each session. `"auto"` uses `$SHELL`, then the account's login shell from `/etc/passwd`, then `/bin/sh`. |
| `user` | `"anth"` | Account the shell runs as. `"auto"` uses the daemon's effective user. |
| `token` | `""` | Reserved for the authentication layer; currently ignored. |
| `max_sessions` | `4` | Concurrent sessions; further opens are refused, not queued. |

When `user` differs from the daemon user, the daemon must run as root and
`setpriv` (from util-linux) must be installed. The Docker image includes it and
runs as `anth`, so the default requires no switch. A debug build bootstraps a
missing config with `user = "auto"` so it can run as the local account.

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
- **The child gets a minimal environment**: `TERM`, `PATH`, `HOME`, `USER`,
  `LOGNAME`, `SHELL`, `LANG` and `LC_ALL`. `HOME`, `USER` and `LOGNAME` belong to
  the configured terminal user. The locale is forced to `C.UTF-8` so tools such
  as `btop` can start; the daemon's own environment is not inherited, so
  credentials in it do not leak into a shell.
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
