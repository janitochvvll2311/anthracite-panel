# Command line

`anth-panel` is a single binary with four roles: it can serve the daemon, serve
the HTTP API, do both, or act as a client that talks to a running daemon.

```
Usage: anth-panel [OPTIONS] [COMMAND]

Commands:
  daemon     Serve the Unix-socket daemon
  api        Serve the HTTP API and the embedded web UI
  run        Serve the daemon and the HTTP API from a single process
  echo       Send a request to the running daemon and print the response
  host-info  Print the operating system and resources of the daemon host as a table
  process    Inspect processes and send signals
  network    Inspect network interfaces and change their state
  firewall   Inspect and configure the managed nftables firewall
  file       Browse and manage files on the daemon host
  terminal   Open an interactive terminal on the daemon host
  help       Print this message or the help of the given subcommand(s)

Options:
  -c, --config <FILE>  Path to the configuration file
  -i, --interactive    Read commands from a prompt on stdin instead of running one
  -h, --help           Print help
  -V, --version        Print version
```

`[COMMAND]` is optional only because `-i` alone is valid; without `-i` a bare
invocation exits with an error. See [Interactive sessions](#interactive-sessions).

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
sudo = false

[process]
control = false

[network]
control = false
manage = false

[firewall]
control = false

[auth]
enabled = true
username = "admin"
password_hash = "pbkdf2-sha256$..."
jwt_secret = "..."
access_token_secs = 900
refresh_token_secs = 43200
cookie_secure = false

[files]
root = "/"
write = false
max_entries = 5000
max_preview_bytes = 1048576
max_upload_bytes = 67108864
```

Unknown keys are rejected rather than ignored, so a typo fails loudly. The
legacy `network.sudo`, `firewall.sudo` and `firewall.sudo_password` keys are
accepted for compatibility, ignored and omitted when the config is written; use
the top-level `sudo` instead. A relative or missing file is not fatal for
serving subcommands: they write a default config when it cannot be loaded.
Client subcommands (`echo`, `host-info`, `process`, `network`, `firewall`,
`file`) never create the file — they use defaults if it is absent, so probing a
machine does not leave files behind.

The first run of a serving subcommand with `[auth]` enabled but no password
generates one: it prints the account name and the password to the terminal
once, stores only the PBKDF2 digest and a generated signing key, and never
shows the password again. Set a new one with `anth-panel auth set-password`.

The file is written with mode `0600` because it holds the digest and the signing
key. A local account that can read the key could forge an access token, so the
mode is set before the content is written and tightened again on every save.

Debug builds bootstrap a missing file with global `sudo = true`,
`[process] control = true`, `[network] control = true`,
`[network] manage = true`, `[firewall] control = true` and
`[files] write = true` so the whole UI is testable locally. Release builds keep the controls off. An existing file is
never rewritten; remove it or edit it to change the generated defaults.

## Serving

### `daemon`

Binds the Unix socket and answers requests. Blocks until interrupted.

```bash
anth-panel daemon
```

Prints `Listening on <path>` once bound. Any previous socket file at that path is
removed first, so a stale file from a crashed process does not block startup.

### Interactive sessions

`-i` / `--interactive` is global: it works before or after the subcommand, and
it turns the invocation into a prompt that runs commands one after another
instead of exiting after one.

```bash
anth-panel -i                     # bare prompt
anth-panel --interactive          # same
anth-panel -i host-info           # print the snapshot, then stay at the prompt
anth-panel host-info -i           # same
```

A bare invocation without `-i` has nothing to run and exits with an error, since
`-i` alone is valid and clap therefore cannot require the subcommand.

```
$ anth-panel -i
Type a command without the `anth-panel` prefix, or `help`. Ctrl-D or `quit` exits.
> echo "hola mundo"
[OK] hola mundo
> host-info
┌──────────────────┬─────────────────────────┐
│ FIELD            │ VALUE                   │
...
> quit
```

Each line is parsed by the binary's own clap definition, executed by the same
dispatcher a one-shot invocation uses, and rendered by the same table renderer,
so a typed command behaves exactly like the subcommand: flags, defaults,
confirmation prompts and error text included. `-i` is itself accepted on a typed
line, so pasting a full command line works.

`help` (or `?`) prints the command list. `quit` or `exit`, and Ctrl-D, close the
prompt. A failed command reports its error and the prompt continues; an empty
line is ignored. Quotes and backslashes group arguments, so
`echo "hola mundo"` sends one message rather than two words; there is no
expansion, pipeline or redirection.

A watch flag is accepted but prints one frame: a stream would hold the prompt
until interrupted, which a typed command should not do. A client session exits
with `0` even after a failed command, because the session itself completed.

`daemon`, `api` and `run` are detached instead of blocking the prompt, so one
terminal can serve and still send requests:

```
$ anth-panel -i
Type a command without the `anth-panel` prefix, or `help`. Ctrl-D or `quit` exits.
> daemon
Listening on /var/tmp/anth.sock
[background] daemon started; the prompt stays available, `quit` stops it
> api
Server is listening on 0.0.0.0:8080
[background] api started; the prompt stays available, `quit` stops it
> echo hola
[OK] hola
```

The session waits until the port is bound before reporting the start, so the
next typed line cannot race the bind. `daemon` and `api` can run together;
starting the same service twice is refused, and `run` conflicts with either
because it binds both. `quit`, `exit` or Ctrl-D stops every detached server;
a daemon started in another process is unaffected.

`terminal` is refused inside a session because it needs the terminal's own
stdin, which the line reader owns; run it as a separate invocation,
`anth-panel terminal`. `auth` and `config` do run in a session, but note that
they edit the config file in this process rather than through the daemon.

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

#### `process spawn <PROGRAM> [ARGS...]`

```bash
anth-panel process spawn /bin/ls -la /tmp
anth-panel process spawn --cwd /tmp --yes /usr/bin/uptime --pretty
```

`PROGRAM` must be an absolute path that appears in `[process] spawn_allow`, and
`[process] spawn` must be `true`. Everything after `PROGRAM` is passed as an
argument vector, not through a shell, so quoting on the command line is the
shell's job and nothing is re-split. That also means `--cwd` and `--yes` must
come **before** `PROGRAM`; after it they are arguments to the program. Use `--`
to pass an argument that starts with a hyphen.

`--cwd DIR` sets the working directory. The output of the started process is
discarded and the daemon does not supervise it; the command prints the new pid
and exits.

Without `--yes` the command asks for confirmation on a TTY and refuses to run
when stdin is not a terminal.

### `network`

Inspects network interfaces and, when `[network] control = true`, brings one up
or down; when `[network] manage = true`, also creates, edits, renames and deletes
them. Both switches are off by default; see [network.md](network.md) for the
policy, data sources and safety limits.

#### `network list [--watch]`

```bash
anth-panel network list
anth-panel network ls
anth-panel network list --watch 2s
```

Prints IFACE, STATE, ADDRESS, SPEED, RX and TX. `ls` is an alias for `list`.
`--watch` takes the same interval rules as `host-info --watch`.

RX and TX are cumulative counters, not rates. The web monitor computes rates
between snapshots.

#### `network up <INTERFACE>` and `network down <INTERFACE>`

```bash
anth-panel network up eth0
anth-panel network down eth0
anth-panel network down eth0 --yes
```

Without `--yes`, the command asks for confirmation on a TTY and refuses to run
when stdin is not a terminal. The daemon refuses changes when
`[network] control = false`, rejects invalid or unknown interface names, and
will not bring a loopback interface down. The daemon user needs `CAP_NET_ADMIN` for the kernel
operation to succeed.

#### `network create <NAME> --kind <KIND>`

```bash
anth-panel network create br0 --kind bridge --mtu 1500 --up
anth-panel network create veth0 --kind veth --peer veth1
anth-panel network create eth0.10 --kind vlan --link eth0 --vlan-id 10
```

Creates one virtual interface. Kinds are `bridge`, `bond`, `dummy`, `veth`,
`vlan` and `tun-tap`; a physical device cannot be created. `--peer` belongs to
`veth`, `--link` and `--vlan-id` to `vlan`, and `--mtu` must be 68-65535.
Without `--yes` the command confirms first. It exits `1` when the daemon refuses
the request.

#### `network edit <INTERFACE> [--mtu <MTU>] [--up|--down]`

```bash
anth-panel network edit eth0 --mtu 9000
anth-panel network edit eth0 --down --yes
```

Changes only the fields that are given, so an MTU edit leaves the link state
alone. At least one of `--mtu`, `--up` and `--down` is required.

#### `network rename <INTERFACE> <NEW_NAME>`

```bash
anth-panel network rename d0 d1
```

Renames an interface. It must be down, or the daemon answers `1`. Names are
limited to 15 bytes with no whitespace, slash or colon.

#### `network delete <INTERFACE>`

```bash
anth-panel network delete d0 --yes
```

Deletes an interface, including a physical device; the confirmation names the
device so the operator knows the host loses that link. The daemon still refuses
a loopback, the last interface carrying a default route, and — over HTTP — the
interface the request arrived through.

### `firewall`

Inspects and, when `[firewall] control = true`, changes the dedicated
`inet anthracite` nftables table. See [firewall.md](firewall.md) for the table
layout, requirements and permissions.

#### `firewall status`

```bash
anth-panel firewall status
anth-panel firewall list
```

Prints the managed rules as HANDLE, ACTION, PROTO, PORT and COMMENT. Reading
does not require control, but it does require permission to run `nft`.
`list` is an alias for `status`. Only the managed `input` chain is shown, and a
snapshot shortened to fit the protocol line cap is followed by a `Warning:` line
that states how many rules were kept.

#### `firewall allow <PORT[/PROTOCOL]>` and `firewall deny ...`

```bash
anth-panel firewall allow 22/tcp
anth-panel firewall allow 53/udp
anth-panel firewall allow 8080          # tcp by default
anth-panel firewall deny 23/tcp
```

`deny` adds a drop rule to the managed blocklist. `allow` removes a matching
managed drop rule; it cannot override a different firewall table. If no matching
deny exists, the command reports that traffic is already allowed.

Without `--yes`, the command asks for confirmation on a TTY and refuses to run
when stdin is not a terminal. The daemon must have `[firewall] control = true`
and enough privilege to run `nft` directly (`CAP_NET_ADMIN`, root, or the
container file capability).

#### `firewall delete <HANDLE>` and `firewall flush`

```bash
anth-panel firewall delete 17
anth-panel firewall flush
```

`delete` removes one rule by the handle shown in `status`. `flush` removes every
rule from the managed chain. Both ask for confirmation unless `--yes` is given.

### `volume`

Inspects the storage inventory and, when `[volumes] control = true`, mounts and
unmounts filesystems. See [volumes.md](volumes.md) for the data sources, the
mount and unmount policy and the error kinds.

#### `volume list`

```bash
anth-panel volume list
anth-panel volume ls
```

Prints two tables. The first lists block devices as DEVICE, SIZE, TYPE, FSTYPE,
LABEL and MOUNT, indented by their position in the tree. The second lists every
mount as MOUNTPOINT, SOURCE, FSTYPE, USED, AVAILABLE and MODE, where MODE is
`rw`, `ro`, `virtual` or `ro, virtual`. `ls` is an alias. A snapshot shortened to
fit the protocol line cap is followed by a `Warning:` line.

#### `volume mount <DEVICE> <TARGET>`

```bash
anth-panel volume mount /dev/sdb1 /mnt/data
anth-panel volume mount sdb1 /mnt/data --type ext4
anth-panel volume mount /dev/sdb1 /mnt/backup --read-only
anth-panel volume mount /dev/sdc1 /mnt/usb --allow-removable
```

`DEVICE` is a full path or a kernel name. `TARGET` must already exist and resolve
inside `volumes.mount_root`. `--type` overrides the detected filesystem.
`--read-only` mounts with `-o ro`. `--allow-removable` is required for removable
media, which is refused otherwise.

Without `--yes`, the command asks for confirmation on a TTY and refuses to run
when stdin is not a terminal. The daemon must have `[volumes] control = true`
and enough privilege to run `mount`.

#### `volume unmount <TARGET>`

```bash
anth-panel volume unmount /mnt/data
anth-panel volume umount /mnt/data
```

`umount` is an alias. `/`, virtual filesystems and any mount that contains the
daemon's socket, config, served root or executable are refused. A busy
filesystem reports an error; there is no lazy or forced unmount. The command
asks for confirmation unless `--yes` is given.

### `auth`

These edit the config file directly; they do not need a running daemon.

| Command | Description |
| --- | --- |
| `auth status` | Report whether auth is on and whether a password is set. |
| `auth enable` | Turn the password requirement on. |
| `auth enable --disable` | Turn it off. |
| `auth set-password` | Read a password (prompt, or stdin with `--stdin`) and store its digest. |

`auth set-password` prompts twice on a terminal, with echo off, and reads one
line from stdin with `--stdin`. The password never appears in the process list
because it is never an argument. Changing it restarts the panel's sessions:
they live in memory, so a restart signs everyone out anyway.

### `config`

| Command | Description |
| --- | --- |
| `config get` | Print the effective configuration as TOML. |
| `config set <KEY> <VALUE>` | Change one key and save the file. |

`config set` validates the value before writing, so a bad path or a
non-numeric limit fails with an error and leaves the file untouched. It accepts
the same dotted keys the file uses, such as `files.write` or
`volumes.mount_root`.

The panel's settings page edits the same fields through `PATCH /api/config`
while the process runs; the CLI is for a host with no browser.

### `plugin`

Installs and manages plugin bundles. The commands edit the bundle directory and
the `[plugins] grants` in the config, so they run locally; a running server
reads the grants at start and the command prints a reminder to restart it.

| Command | Description |
| --- | --- |
| `plugin list` | List installed plugins with their state, capabilities and UI slots. |
| `plugin install <BUNDLE>` | Install a zip and record its grant. |
| `plugin remove <ID>` | Delete the bundle and forget its grant. |
| `plugin enable <ID>` | Turn a disabled plugin back on. |
| `plugin enable <ID> --disable` | Stop a plugin from loading, keeping it installed. |
| `plugin approve <ID> --accept-capabilities` | Re-read a bundle and record its current capabilities. |

A bundle is a zip holding `plugin.toml`, `plugin.wasm` and an optional `web/`
directory. Anything else is refused rather than skipped. The install validates
the manifest against host policy, hashes the module and writes the *validated*
capability list to the config:

```toml
[[plugins.grants]]
id = "org.anth.wireguard"
version = "0.1.0"
wasm_sha256 = "..."
capabilities = ["wg.status"]
enabled = true
```

That grant is the authority. At every load the module is re-hashed; a mismatch
disables the plugin and leaves it with no capabilities, whatever its manifest
says. A version bump that declares new capabilities needs
`plugin approve --accept-capabilities`, which is the only command that widens a
grant.

A plugin never sends an argv. It names a capability, and the host renders the
argv from the template frozen at install, validating each value with a
validator the host owns. See [design/plugins.md](design/plugins.md) for the
schema, the validators and the threat table.

`plugin list` also prints a `[WARN]` line for a directory that has no grant or
whose module no longer matches. A plugin that fails to load is reported, never
silently skipped.

### `account`

Inspects the host's users and groups and, when `[accounts] control = true`,
changes local ones. See [accounts.md](accounts.md) for the guard matrix, the
error kinds and the container's root requirement.

#### `account list`

```bash
anth-panel account list
anth-panel account ls
```

Prints two tables. The first lists users as USER, UID, GID, GROUP, STATE, SHELL
and SOURCE. STATE is `locked`, `unlocked`, or `?` when `/etc/shadow` could not
be read. The second lists groups as GROUP, GID, MEMBERS and SOURCE. SOURCE is
`local` for an entry the panel can change and `nss` for one resolved from a
directory service, which is read-only. `ls` is an alias.

#### `account user create <NAME>`

```bash
anth-panel account user create alice
anth-panel account user create alice --comment "Alice Example" --shell /bin/bash
anth-panel account user create alice --uid 1500 --gid 1500
anth-panel account user create svc --system
```

`--home` defaults to `/home/<NAME>` and must resolve inside
`accounts.home_root`. `--shell` must be on the allowlist. `--system` requires
`accounts.allow_system`. The new account is **locked**: it cannot log in until
a password is set with `account user password`.

#### `account user modify <NAME>`

```bash
anth-panel account user modify alice --comment "Alice B"
anth-panel account user modify alice --lock
anth-panel account user modify alice --unlock
anth-panel account user modify alice --shell /bin/sh
```

Only the flags given are changed. `--lock` and `--unlock` are mutually
exclusive. The home directory is never moved, only re-pointed.

#### `account user delete <NAME>`

```bash
anth-panel account user delete alice
anth-panel account user delete alice --remove-home --yes
```

The home directory is kept unless `--remove-home` is passed, and then it must
still resolve inside `accounts.home_root`. Without `--yes` the command asks for
confirmation on a TTY and refuses to run when stdin is not a terminal.

#### `account user password <NAME>`

```bash
anth-panel account user password alice
printf '%s' "$NEW_PASSWORD" | anth-panel account user password alice --stdin
```

Prompts twice with echo off on `/dev/tty`, or reads the password from stdin with
`--stdin`. The password is never accepted as a flag, because argv is readable
by every account on the host. Requires `[accounts] manage_passwords = true`,
which is separate from `control`. Without `--yes` the command asks for
confirmation first.

#### `account group create <NAME>`

```bash
anth-panel account group create developers
anth-panel account group create developers --gid 2000
```

#### `account group modify <NAME>`

```bash
anth-panel account group modify developers --members alice,bob
anth-panel account group modify developers --gid 2100
```

`--members` **replaces** the member list, so a retried request is idempotent and
there is no separate "add" and "remove". Changing the members asks for
confirmation unless `--yes`.

#### `account group delete <NAME>`

```bash
anth-panel account group delete developers --yes
```

A group that is somebody's primary group is refused.

### `file`

Browses and, when `[files] write = true`, manages files below the configured
root. Paths are virtual: `/` means `[files] root`, not necessarily the host's
real root. See [files.md](files.md) for the containment and symlink policy.

There are deliberately no aliases: the subcommands are `list`, `read`, `mkdir`,
`upload`, `rename`, `delete`, `trash`, `trash-bin`, `archive`, `unarchive` and
`write`.

`delete` is the permanent, unconditional removal. `trash` is the reversible
one and is what the panel uses when `[files] trash = true`; both are explicit so
a script cannot mistake one for the other.

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

Deletes one file, symlink or empty directory **permanently**. A non-empty
directory requires `--recursive`. Without `--yes`, the command asks for
confirmation on a TTY and refuses to run when stdin is not a terminal. Use
`trash` when the entry should be recoverable.

#### `file trash <PATH>`

Moves one entry to the freedesktop trash, keeping its original path so it can be
restored. Requires `[files] trash = true`; the trash root is the one belonging
to the daemon's account. Prints the identifier `trash-bin restore` takes.

#### `file trash-bin <list|restore|empty>`

```bash
anth-panel file trash-bin list
anth-panel file trash-bin restore <ID> [--overwrite] [--yes]
anth-panel file trash-bin empty [--yes]
```

`list` prints the id, name, original path, size and deletion time of every entry
the daemon can see, plus how much space the trash holds against its retention
ceiling. `restore` puts one entry back at its recorded path; `--overwrite`
replaces whatever now occupies it. `empty` deletes every entry for good.
`restore` and `empty` can destroy data, so both ask for confirmation unless
`--yes` is given.

#### `file archive <PATH> [-o OUTPUT] [-f FORMAT]`

Compresses one entry. Format is inferred from `--output`; without `--output` the
archive sits beside the entry with a suffix for that format. `zip`, `tar`,
`tar.gz`, `tar.xz` and `7z` can be extracted, but only the first four can be
written: there is no 7z encoder.

#### `file unarchive <PATH> [--into DIR] [--list]`

Extracts an archive. `--into` defaults to the archive path with its suffix
removed. `--list` prints the members without writing anything and conflicts
with `--into`. Extraction refuses absolute members, `..`, backslashes and any
entry that would write through a symlink, and stops at the `[files]
max_archive_bytes` and entry-count limits.

#### `file write <PATH> <SOURCE> [--expected-modified-ms MILLIS]`

Replaces a text file with a local file, or with stdin when `SOURCE` is `-`.
The file must already exist and the save is atomic. `--expected-modified-ms`
turns the save into an optimistic one: if the file changed since that
timestamp the command fails instead of overwriting, which is how the editor
avoids clobbering a concurrent edit.

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
