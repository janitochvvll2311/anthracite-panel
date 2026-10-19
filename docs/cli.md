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
  process    Inspect processes and send signals
  file       Browse and manage files on the daemon host
  terminal   Open an interactive terminal on the daemon host
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

[process]
control = false

[files]
root = "/"
write = false
max_entries = 5000
max_preview_bytes = 1048576
max_upload_bytes = 67108864
```

Unknown keys are rejected rather than ignored, so a typo fails loudly. A
relative or missing file is not fatal for serving subcommands: they write a
default config when it cannot be loaded. Client subcommands (`echo`,
`host-info`, `process`, `file`) never create the file — they use defaults if it
is absent, so probing a machine does not leave files behind.

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

### `process`

Inspects the process table and, when `[process] control = true`, sends a signal
to one process. See [processes.md](processes.md) for the policy and safety
guards.

#### `process list [--watch]`

```bash
anth-panel process list
anth-panel process ls
anth-panel process list --watch 2s
```

Prints PID, PPID, USER, CPU, MEM, STATE and COMMAND. `ls` is an alias for
`list`. `--watch` takes the same interval rules as `host-info --watch`.

CPU in this table is accumulated CPU time, not a live percentage. The web
monitor computes percentages between snapshots.

If the daemon truncated the snapshot, the table is followed by a `Warning:`
line explaining how many processes were shown and whether command lines were
shortened.

#### `process signal <PID> <SIGNAL>`

```bash
anth-panel process signal 1234 term
anth-panel process signal 1234 kill --yes
```

Allowed signals: `term`, `kill`, `hup`, `int`, `quit`, `stop`, `cont`. Names
are case-insensitive.

Without `--yes`, the command asks for confirmation on a TTY and refuses to run
when stdin is not a terminal. The daemon still enforces its own policy:
`[process] control` must be `true`, the target must belong to the daemon user,
and pid 1 or the daemon itself are rejected.

### `file`

Browses and, when `[files] write = true`, manages files below the configured
root. Paths are virtual: `/` means `[files] root`, not necessarily the host's
real root. See [files.md](files.md) for the containment and symlink policy.

There are deliberately no aliases: the subcommands are `list`, `read`, `mkdir`,
`upload`, `rename` and `delete`.

#### `file list [PATH]`

```bash
anth-panel file list
anth-panel file list /var/log
```

Prints TYPE, NAME, SIZE and MODIFIED. The default path is `/`. Hidden entries
are included; the web component filters them with a toggle.

#### `file read <PATH> [--output <FILE>]`

```bash
anth-panel file read /etc/hostname
anth-panel file read /var/log/syslog --output ./syslog
```

Reads in protocol chunks and writes the bytes to stdout, or to `--output` when
given. A directory is rejected.

#### `file mkdir <PATH> [--parents]`

Creates one directory. `--parents` creates missing intermediate directories.

#### `file upload <LOCAL> <DEST> [--overwrite]`

Uploads `LOCAL` in chunks. The destination must be inside the configured root.
Without `--overwrite`, an existing destination is refused.

#### `file rename <FROM> <TO>`

Renames one entry without overwriting an existing destination.

#### `file delete <PATH> [--recursive] [--yes]`

Deletes one file, symlink or empty directory. A non-empty directory requires
`--recursive`. Without `--yes`, the command asks for confirmation on a TTY and
refuses to run when stdin is not a terminal.

### `terminal`

Opens an interactive terminal on the host the daemon runs on, over the socket
protocol documented in [terminal.md](terminal.md). stdin bytes are forwarded to
the remote shell and its output is written to stdout.

```bash
anth-panel terminal
anth-panel terminal --cols 120 --rows 40
```

| Option | Description |
| --- | --- |
| `--cols <COLS>` | Initial width in columns. Defaults to the local terminal width. |
| `--rows <ROWS>` | Initial height in rows. Defaults to the local terminal height. |

The token is reserved for the pending authentication layer and is not enforced
yet; the daemon currently accepts terminal sessions regardless of it.

```bash
# In a terminal: Ctrl-C reaches the remote shell, not this command.
anth-panel terminal

# With piped input, end with `exit` so the shell ends and the command returns.
printf 'hostname\nexit\n' | anth-panel terminal
```

| Property | Behaviour |
| --- | --- |
| Initial size | `--cols`/`--rows`, else the local terminal, else 80x24 |
| Resize | A `SIGWINCH` on the local terminal resizes the remote PTY |
| Raw mode | Enabled on stdin, so keys and Ctrl-C pass through uninterpreted |
| Exit code | The remote shell's exit status (`0` on clean exit, `1` on a refused session or connection failure) |

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

# Open a shell on the daemon host
anth-panel terminal
```
