# Command line

`anth-panel` is one binary: it can serve the Unix socket, the HTTP API, or both,
and it is also the client.

## Global options

| Option | Meaning |
| --- | --- |
| `--config FILE` | Configuration file. Defaults to `./config.toml` in debug and `/etc/anth/config.toml` in release. `ANTH_CONFIG` is the environment fallback. |
| `--log-level LEVEL` | `error`, `warn`, `info`, `debug` or `trace`. `ANTH_LOG_LEVEL`. |
| `--log-format FORMAT` | `human`, `json` or `off`. `ANTH_LOG_FORMAT`. |
| `--log-out DEST` | `stdout` or `stderr`. `ANTH_LOG_OUT`. |

Flags win over environment, which wins over the `[log]` section, which wins over
the default.

## `run`

```bash
anth-panel run [--addr ADDR] [--allow-reboot]
```

Starts the Unix-socket daemon and the HTTP API in one process. `ADDR` defaults to
`0.0.0.0:8080`.

`--allow-reboot` is required for `reboot` to take the host down. Without it the
daemon answers `403 forbidden` instead of acting.

The server creates a missing config with `Config::load_or_create`. A config that
exists and will not parse is refused, never replaced.

First-run bootstrap respects `[auth] bootstrap`:

- `auto` is `web` in debug and `generated` in release.
- `generated` prints a random password once to stderr. The config stores only
  its hash.
- `web` leaves registration open until the first `POST /api/auth/register`.
- Both apply only while no password hash exists.

## `daemon`

```bash
anth-panel daemon [--allow-reboot]
```

Serves the Unix socket only. It uses the same config and auth bootstrap as
`run`.

## `restart`

```bash
anth-panel restart
```

Replaces the running daemon with a fresh copy of the same command line. The pid
is kept, and the new image re-reads `config.toml`, so this is how a settings
change takes effect. The command answers before the replacement starts.

## `reboot`

```bash
anth-panel reboot
anth-panel reboot --delay 60
```

Asks the host to reboot, optionally after a delay of at most 300 seconds. The
daemon refuses unless it was started with `--allow-reboot`, and the answer
reaches the caller before the host goes down.

## `host-info`

```bash
anth-panel host-info
anth-panel host-info --watch
anth-panel host-info --watch 2s
anth-panel host-info --watch 500ms
```

Prints one snapshot by default, or repeats on a period with `--watch`. The
interval needs an `ms`, `s`, `m` or `h` suffix and is at least `10ms`. A bare
`--watch` uses `2s`.

The output groups values like the web card:

```text
Anthracite Panel · host.example
Debian 13 · kernel 6.12.111 · x86_64 · up 3d 4h

CPU
  Example CPU
  8 logical cores · load 0.12 0.20 0.31

Memory
  3.4 GiB / 7.6 GiB used (45.0%) · 4.2 GiB available

Swap
  not configured
```

On a terminal each watch frame overwrites the previous one. When the output is
redirected, frames are separated by a blank line. `Ctrl-C` exits `0`; a dropped
daemon exits `1`.

## `files`

```bash
anth-panel files list
anth-panel files list --path /etc --limit 50
anth-panel files list --path /var/log --cursor <opaque>
anth-panel files read --path /etc/hosts
anth-panel files read --path big.log --offset 4096 --max-bytes 8192
anth-panel files delete --path /tmp/old.log
anth-panel files write --path /tmp/motd < motd.txt
anth-panel files write --path /tmp/note.txt --create < note.txt
anth-panel files upload --path /tmp/logo.png --create < logo.png
```

`list` reads one page of a directory under `[files] root` (`/` by default),
sorted by name. `--limit` is 1–1000; `--cursor` is the `next` value from the
previous page. `read` prints a UTF-8 text file, capped by
`files.max_preview_bytes`; when it stops early, the next offset is printed to
stderr. `delete` removes one file or symlink and requires `files.write = true`;
directories and the root are refused. `write` reads UTF-8 content from stdin
(up to `files.max_upload_bytes`) and requires `files.write`; `--create` fails
instead of replacing an existing file. `upload` sends any bytes from stdin
through the same write path; the daemon caps the decoded size with
`files.max_upload_bytes`. Paths are relative to the root; `..` and paths that
escape it through a symlink are refused.

## `config`

```bash
anth-panel config get
anth-panel config validate
anth-panel config set KEY VALUE
```

- `get` prints the file.
- `validate` parses and validates it without writing.
- `set` changes one dotted key and parses the result before the atomic write.
  Comments and key order are preserved. A bad value leaves the file untouched.

```bash
anth-panel config set files.write true
anth-panel config set auth.username '"operator"'
```

Values are TOML literals, so strings need quotes.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | success |
| `1` | runtime, daemon, transport or configuration error |
| `2` | invalid usage or invalid request value |

## Examples

```bash
# development, debug config and relative paths
cd backend
cargo run -p anth-panel -- run --addr 127.0.0.1:8080 --config ./config.toml

# one snapshot against that daemon
cargo run -p anth-panel -- host-info --config ./config.toml

# follow the host
cargo run -p anth-panel -- host-info --watch 2s --config ./config.toml

# keep the web development server next to it
cd frontend && bun run dev
```
