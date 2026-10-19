# Command line

`anth-panel` is a single binary with four roles: it can serve the daemon, serve
the HTTP API, do both, or act as a client that talks to a running daemon.

```
Usage: anth-panel [OPTIONS] <COMMAND>

Commands:
  daemon     Serve the Unix-socket daemon
  api        Serve the HTTP API and the embedded web UI
  run        Serve the daemon and the HTTP API from a single process
  echo       Send a request to the running daemon and print the response
  host-info  Print the operating system and resources of the daemon host as a table
  help       Print this message or the help of the given subcommand(s)

Options:
  -c, --config <FILE>  Path to the configuration file
  -h, --help           Print help
  -V, --version        Print version
```

## Global options

| Option | Description |
| --- | --- |
| `-c`, `--config <FILE>` | Config file to use. Defaults to `/etc/anth/config.toml`. |
| `-h`, `--help` | Print help. |
| `-V`, `--version` | Print the version (`0.1.0`). |

`--config` is global, so it can appear before or after the subcommand:

```bash
anth-panel --config /tmp/anth.toml host-info
anth-panel host-info --config /tmp/anth.toml
```

Both forms are equivalent.

### Configuration file

```toml
socket_path = "/var/tmp/anth.sock"
```

Unknown keys are rejected rather than ignored, so a typo fails loudly. A
relative or missing file is not fatal for serving subcommands: they write a
default config when it cannot be loaded. Client subcommands (`echo`,
`host-info`) never create the file — they use defaults if it is absent, so
probing a machine does not leave files behind.

## Serving

### `daemon`

Binds the Unix socket and answers requests. Blocks until interrupted.

```bash
anth-panel daemon
```

Prints `Listening on <path>` once bound. Any previous socket file at that path is
removed first, so a stale file from a crashed process does not block startup.

### `api`

Serves the HTTP API and the embedded web UI. Blocks until interrupted.

```bash
anth-panel api
anth-panel api --addr 127.0.0.1:9000
```

| Option | Description |
| --- | --- |
| `--addr <ADDR>` | Bind address. Falls back to `ANTH_PANEL_ADDR`, then `0.0.0.0:8080`. |

The API needs a daemon to be useful, but it does not start one. Routes are
documented in [api-endpoints.md](api-endpoints.md).

### `run`

Starts the daemon and the API in one process. Blocks until interrupted.

```bash
anth-panel run
anth-panel run --addr 127.0.0.1:9000
```

Takes the same `--addr` option as `api`. The HTTP port is bound before the
socket, so an address already in use fails immediately instead of leaving a
socket behind. This is the mode the Docker image uses.

## Client commands

These connect to a running daemon. With no daemon listening they fail with
`Failed to connect to socket` and exit `1`.

### `echo <MESSAGE>`

Sends a message and prints the daemon's reply.

```bash
$ anth-panel echo "hola"
[OK] hola
```

| Argument | Description |
| --- | --- |
| `<MESSAGE>` | Text to send. Required. |

### `host-info`

Prints the operating system and resources of the host the daemon runs on, as a
table.

```
$ anth-panel host-info
┌──────────────────┬────────────────────────────────────────┐
│ FIELD            │ VALUE                                  │
├──────────────────┼────────────────────────────────────────┤
│ Hostname         │ 8721a005a5d3                           │
│ Operating system │ Debian GNU/Linux 13                    │
│ Kernel           │ 6.12.111+deb13-amd64                   │
│ Architecture     │ x86_64                                 │
├──────────────────┼────────────────────────────────────────┤
│ CPU              │ Intel Core Processor (Haswell, no TSX) │
│ Logical cores    │ 4                                      │
│ Uptime           │ 6d 12h 33m 40s                         │
│ Load average     │ 0.60 / 0.51 / 0.39                     │
│ Memory total     │ 7.6 GiB                                │
│ Memory used      │ 3.4 GiB (45%)                          │
│ Memory available │ 4.1 GiB (54%)                          │
│ Swap             │ not configured                         │
└──────────────────┴────────────────────────────────────────┘
```

The table is the same data the API returns as JSON; see
[socket-protocol.md](socket-protocol.md#hostinfo-reply) for field meanings.

#### `--watch [INTERVAL]`

Repeats the read until interrupted.

```bash
anth-panel host-info --watch           # every 2s
anth-panel host-info --watch 500ms     # every 500ms
anth-panel host-info --watch 1m        # every minute
```

| Property | Behaviour |
| --- | --- |
| Default | `2s` when `--watch` is given without a value |
| Units | `ms`, `s`, `m`, `h`; fractional values allowed (`1.5s`) |
| Minimum | `10ms`. Below that the command exits `1` with an error |
| Maximum | `24h` |
| First read | Immediate, then one read per interval |
| Connection | One socket for the whole stream |
| On a terminal | Each frame overwrites the previous one |
| Redirected | Frames are separated by a blank line |
| Ctrl-C | Exits `0` |
| Daemon gone | Exits `1` |

```bash
anth-panel host-info --watch 1s | tee -a host.log
```

Redirected output keeps every frame, which is what you want for a log.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, including a stream stopped with Ctrl-C |
| `1` | Bad arguments, unreachable daemon, or an error response |

## Examples

```bash
# Serve everything on a custom config
anth-panel run --config ./dev.toml --addr 127.0.0.1:8080

# Check a host once
anth-panel host-info

# Follow a host's memory for a minute
timeout 60 anth-panel host-info --watch 2s > memory.log

# Wake the daemon with a round trip
anth-panel echo ping
```
