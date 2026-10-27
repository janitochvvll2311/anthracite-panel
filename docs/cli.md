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
anth-panel files trash --path /tmp/old.log
anth-panel files trash-bin list
anth-panel files trash-bin restore old.log
anth-panel files trash-bin restore old.log --overwrite
anth-panel files trash-bin empty
anth-panel files archive --path /var/log
anth-panel files archive --path /var/log --format zip --destination /tmp/log.zip
anth-panel files unarchive --path /var/log.tar.gz --list
anth-panel files unarchive --path /var/log.tar.gz
```

`list` reads one page of a directory under `[files] root` (`/` by default),
with directories first and then the rest, each group sorted by name. `--limit`
is 1–1000; `--cursor` is the `next` value from the previous page. `read` prints
a UTF-8 text file, capped by
`files.max_preview_bytes`; when it stops early, the next offset is printed to
stderr. `delete` removes one file or symlink and requires `files.write = true`;
directories and the root are refused. `write` reads UTF-8 content from stdin
(up to `files.max_upload_bytes`) and requires `files.write`; `--create` fails
instead of replacing an existing file. `upload` sends any bytes from stdin
through the same write path; the daemon caps the decoded size with
`files.max_upload_bytes`. Paths are relative to the root; `..` and paths that
escape it through a symlink are refused.

`trash` moves one entry to the freedesktop.org trash and needs both
`files.write` and `files.trash`; `trash-bin list` prints the identifier, the
original path and the deletion date, `restore` puts one entry back (refusing an
existing target unless `--overwrite`), and `empty` deletes everything the trash
holds. `delete` stays permanent and is never silently converted into a trash
move.

`archive` compresses one entry. The destination defaults beside it and the
format defaults to `tar.gz`; `zip`, `tar`, `tar.gz` and `tar.xz` can be created,
while `7z` is read-only. `unarchive --list` inspects an archive without writing,
and `unarchive` extracts it into the archive's name without the suffix. The
daemon caps the uncompressed size with `files.max_archive_bytes`, counts members
against `files.max_entries`, and refuses absolute members, `..`, backslashes and
writes through archive-created symlinks.

## `terminal`

```bash
anth-panel terminal
anth-panel terminal --cols 100 --rows 30
```

Opens an interactive shell on a pseudo-terminal. It puts the local terminal in
raw mode, forwards window changes, restores the local terminal on exit and
returns the remote exit code. The daemon must have `[terminal] enabled = true`.
The shell is the one configured by `[terminal] shell`; the CLI cannot choose a
different program.

## `network`

```bash
anth-panel network list
anth-panel network ls --watch
anth-panel network ls --watch 2s
anth-panel network up eth0
anth-panel network down eth0
anth-panel network create br0 --kind bridge --mtu 1500 --up --yes
anth-panel network edit eth0 --mtu 1400 --yes
anth-panel network rename eth0 lan0 --yes
anth-panel network delete eth0 --yes
```

`list` (alias `ls`) prints the interfaces, one row per address, then the default
routes. The counters are cumulative bytes; the CLI does not derive rates.
`--watch` repeats the table until Ctrl-C, and a bare `--watch` uses `2s`.

`up` and `down` need `[network] control`. `create`, `edit`, `rename` and
`delete` need `[network] manage`; `edit --up/--down` also needs `control`.
They ask for confirmation on a TTY and refuse without `--yes` when stdin is not
a terminal. `create` accepts `--kind` (`veth`, `bridge`, `bond`, `vlan`,
`dummy` or `tun-tap`), `--peer`, `--link`, `--vlan-id`, `--mtu` and `--up`.
`edit` takes `--mtu`, `--up` or `--down`.
Mutations print the daemon's one-line message. Reading works regardless of both
switches.

## `firewall`

```bash
anth-panel firewall status
anth-panel firewall list
anth-panel firewall deny 22
anth-panel firewall deny 53/udp --chain output --comment dns --yes
anth-panel firewall allow 80/tcp --yes
anth-panel firewall edit 4 443/tcp --action allow --comment https --yes
anth-panel firewall edit 4 --chain output --yes
anth-panel firewall policy input deny --yes
anth-panel firewall delete 4 --yes
anth-panel firewall flush --chain output --yes
```

`status` (alias `list`) prints each chain's policy and then the rules as
HANDLE, CHAIN, ACTION, PROTO, PORT and COMMENT. `deny` and `allow` take
`PORT[/PROTOCOL]`; the protocol defaults to `tcp`, `--chain` defaults to
`input`, and `--comment` is optional. Both actions add a real `accept` or
`drop` rule; adding a rule for an existing chain/protocol/port replaces it.
`edit` changes only the fields given on the command line and can move a rule to
the other chain. `policy` takes `CHAIN POLICY`, where policy is `allow`/`accept`
or `deny`/`drop`, and warns about the lockout risk. Every mutation requires
`[firewall] control = true`, asks for confirmation on a TTY and refuses
without `--yes` when stdin is not a terminal.

## `storage`

```bash
anth-panel storage list
anth-panel storage ls
anth-panel volumes list
```

Prints the policy flags, the block-device tree, the mounts with used/total space
and any active swap. `volumes` is an alias for `storage`. The inventory is
read-only in this slice; a missing `lsblk` or mountinfo file prints a warning
and keeps the other source.

## `process`

```bash
anth-panel process list
anth-panel process ls --limit 20
anth-panel process list --watch 2s
anth-panel process signal 1234 term
anth-panel process signal 1234 kill --yes
anth-panel process signal 1234 term --group
anth-panel process spawn /bin/sleep 30
anth-panel process spawn --cwd /tmp /bin/uptime
```

`list` prints PID, PPID, USER, STATE, TICKS, MEM, THR and COMMAND, busiest
first. `--limit` is 1–1000 and `--cursor` is the `next` value from the previous
page; `--watch` follows the table until Ctrl-C. `signal` looks up the process to
read its `start_ticks`, asks for confirmation on a TTY and refuses without
`--yes` when stdin is not a terminal; `--group` signals the process group led by
the pid. It requires `[process] control = true`. `spawn` starts an absolute
program from `[process] spawn_allow`, passes the remaining words as argv without
a shell, and prints the pid; it requires `[process] spawn = true`. Because the
words are passed verbatim, put `--cwd` before the program.

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
