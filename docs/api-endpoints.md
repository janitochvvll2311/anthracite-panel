# HTTP API

The panel serves JSON over HTTP and SSE. The same `anth-panel` process can also
serve the Unix socket.

## Conventions

- Routes name the command; bodies carry the command's arguments at the top
  level, with no socket `type`/`data` envelope.
- Every API response carries `X-Anth-Protocol: 2`.
- Errors use one shape:

  ```json
  {
    "error": {
      "kind": "forbidden",
      "code": "policy_off",
      "message": "process control is disabled"
    }
  }
  ```

  `kind` is one of `not_found`, `forbidden`, `unauthorized`, `conflict`,
  `invalid`, `unsupported`, `too_large`, `too_many`, `internal`.
- The access token lives in the `anth_access` cookie or an
  `Authorization: Bearer` header. The refresh token lives in `anth_refresh`,
  scoped to `/api/auth`.
- Mutating auth requests also check `Origin` against `Host`.
- An unknown `/api` path is a JSON `404`, never the SPA.

## `GET /healthz`

Public. Returns `{"status":"ok"}`. It does not read config or credentials.

## Authentication

### `GET /api/auth/state`

Public. The shell calls it before rendering a form.

```json
{
  "required": true,
  "registered": true,
  "authenticated": false,
  "bootstrap": "closed",
  "username": "admin"
}
```

- `required`: auth is enabled.
- `registered`: a password hash exists.
- `authenticated`: the request carried a live access token.
- `bootstrap`: `web` while first-run registration is open, otherwise `closed`.

### `POST /api/auth/register`

Public only while `bootstrap = "web"` and no password exists.

```json
{ "password": "at least 12 characters" }
```

- `201` with the access/refresh cookies, or `409 bootstrap_closed` after the
  first registration.
- `400 password_too_short` when the password is under 12 characters.
- `429 too_many` after repeated failures, with `Retry-After` in seconds.

### `POST /api/auth/login`

Public.

```json
{ "username": "admin", "password": "..." }
```

- `200` with cookies and `{"token_type":"Bearer","expires_in":900}`.
- `401 invalid_credentials`.
- `409 not_registered` when no account exists.
- `429 too_many` after five failures from one source in a minute, with
  `Retry-After` in seconds. Only failures spend the budget; a successful login
  clears it.

### `POST /api/auth/refresh`

Uses the `anth_refresh` cookie, or `{ "refresh_token": "..." }` in the body.

- `200` with a new cookie pair.
- `401 invalid_credentials` when the token is unknown, expired or reused.
- A reused token drops every session in its chain.

### `POST /api/auth/logout`

Clears the cookies and drops the operator's sessions. Returns `{"ok":true}`.

### `POST /api/auth/password`

Requires a live access token.

```json
{ "current_password": "...", "new_password": "..." }
```

- `200` and clears the cookies; every session is revoked.
- `401` if the current password is wrong.
- `400 password_too_short`.

### `GET /api/auth/sessions`

Requires a live access token.

```json
{
  "max": 10,
  "sessions": [
    { "sid": "d301d3dab719f6803488bfbec29d8483", "expires": 1795634191, "current": true }
  ]
}
```

One entry per login, not per refresh rotation. `sid` is safe to display: the
refresh token is stored only as a digest. `expires` is Unix seconds.

### `POST /api/auth/sessions/revoke`

Requires a live access token.

```json
{ "others": true }
```

- `{"ok":true,"revoked":1,"scope":"others"}` revokes every session except the
  caller's.
- `{ "sid": "..." }` revokes one session instead.

## Host info

### `GET /api/host-info`

Requires a live access token unless auth is disabled.

```json
{
  "hostname": "example",
  "os": { "name": "Debian GNU/Linux", "version": "13", "kernel": "6.12.0", "arch": "x86_64" },
  "uptime_seconds": 1234,
  "cpu_model": "Example CPU",
  "logical_cores": 8,
  "load": { "one": 0.12, "five": 0.2, "fifteen": 0.31 },
  "memory": {
    "total_kib": 8388608,
    "available_kib": 4194304,
    "swap_total_kib": 0,
    "swap_free_kib": 0
  },
  "collected_at_ms": 1790000000000
}
```

Unreadable fields degrade to `unknown` or `0`.

### `GET /api/host-info/watch?interval=2s`

Requires a live access token. Server-Sent Events:

- `event: host-info`
- `id`: `collected_at_ms`
- `data`: the same `HostInfo` object
- `retry: 3000`
- Keep-alive comments every 15 seconds.

A bad interval is a `400` before the stream opens. After the first byte, a
failure is an `event: error` with the error JSON and the stream closes.
`interval` accepts `ms`, `s`, `m` and `h` suffixes and is at least `10ms`.

## Network

The inventory and its guarded mutations; policy and the contract are in
[network.md](network.md). Every route requires a live access token unless auth
is disabled.

### `GET /api/network`

```json
{
  "interfaces": [
    {
      "name": "eth0",
      "index": 2,
      "kind": "ether",
      "mac": "00:11:22:33:44:55",
      "mtu": 1500,
      "state": "up",
      "up": true,
      "running": true,
      "loopback": false,
      "physical": true,
      "addresses": [
        { "family": "inet", "address": "192.168.1.10", "prefix": 24, "scope": "global" }
      ],
      "rx_bytes": 123456,
      "tx_bytes": 654321,
      "rx_packets": 1000,
      "tx_packets": 900,
      "rx_errors": 0,
      "tx_errors": 0,
      "rx_dropped": 0,
      "tx_dropped": 0,
      "speed_mbps": 1000
    }
  ],
  "routes": [
    { "family": "inet", "gateway": "192.168.1.1", "interface": "eth0" }
  ],
  "timestamp_ms": 1790000000000,
  "control_enabled": false,
  "manage_enabled": false
}
```

Counters are cumulative. `control_enabled` and `manage_enabled` report
`[network] control` and `[network] manage`; the inventory itself is always
readable. `speed_mbps` is omitted when the driver reports none.

### `GET /api/network/watch?interval=2s`

Server-Sent Events, the same shape as `host-info/watch`:

- `event: network`
- `id`: `timestamp_ms`
- `data`: one snapshot
- `retry: 3000`
- keep-alive comments every 15 seconds.

A bad interval is a `400` before the stream opens.

### Network mutations

All five routes are `POST`, require a live session, and answer
`200 {"message":"..."}`. The bodies and guards are in [network.md](network.md).

| Route | Body | Policy |
| --- | --- | --- |
| `/api/network/state` | `{"name":"eth0","up":false}` | `[network] control` |
| `/api/network/create` | `{"name":"br0","kind":"bridge","mtu":1500,"up":false}` | `[network] manage` |
| `/api/network/edit` | `{"name":"eth0","mtu":1400,"up":false}` | `[network] manage`; `up` also needs `control` |
| `/api/network/rename` | `{"name":"eth0","new_name":"lan0"}` | `[network] manage` |
| `/api/network/delete` | `{"name":"eth0"}` | `[network] manage` |

`create` accepts `veth` (with `peer`), `vlan` (with `link` and `vlan_id`),
`bridge`, `bond`, `dummy` and `tun`. The delete route adds the request's peer
address on the server; a `peer` in the body is ignored. Refusals use
`403 network_control_disabled`, `403 network_manage_disabled`, `403
protected_interface`, `409 interface_up`, `409 interface_exists`, `404
interface_not_found`, `400 invalid_interface`, `403 ip_permission`, `501
ip_unsupported`, `501 tun_unavailable` and `500 ip_failed`/`ip_unavailable`.

## Firewall

The managed `inet anthracite` table with its `input` and `output` chains;
policy and semantics are in [firewall.md](firewall.md). All routes require a
live session unless auth is disabled.

### `GET /api/firewall`

```json
{
  "initialized": true,
  "control_enabled": true,
  "rules": [
    {
      "handle": 3,
      "chain": "input",
      "action": "deny",
      "protocol": "tcp",
      "port": 80,
      "comment": "http"
    }
  ],
  "chains": [
    { "chain": "input", "initialized": true, "policy": "deny" },
    { "chain": "output", "initialized": true, "policy": "allow" }
  ],
  "ruleset": "table inet anthracite { ... }",
  "timestamp_ms": 1790000000000
}
```

A missing table is `initialized: false` with no rules, both chains not created
and an empty ruleset.

### `POST /api/firewall/rule`

```json
{ "chain": "input", "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" }
```

`200 {"message":"denied tcp/80 on input"}`. `chain` is `input` or `output` and
defaults to `input`; `action` is `allow` or `deny`; `protocol` is `tcp` or
`udp`. A rule for the same chain/protocol/port replaces the existing one.
Requires `[firewall] control`. Refusals include `403
firewall_control_disabled`, `400 firewall_port`, `400 firewall_comment`, `403
nft_permission`, `501 nft_unavailable` and `400 nft_refused`.

### `POST /api/firewall/update`

```json
{ "handle": 4, "chain": "input", "action": "allow", "protocol": "tcp", "port": 443 }
```

`200 {"message":"updated firewall rule 4 to allow tcp/443 on input"}`. Custom
rules are refused with `400 firewall_custom_rule`; a duplicate target in the
same chain is `400 firewall_duplicate`; an unknown handle is `404
firewall_rule_not_found`.

### `POST /api/firewall/delete`

```json
{ "handle": 4 }
```

`200 {"message":"deleted firewall rule 4"}`. The daemon resolves the rule's
chain. Requires control.

### `POST /api/firewall/flush`

```json
{ "chain": "output" }
```

`200 {"message":"flushed 2 firewall rules from output"}`. `chain` defaults to
`input`. Requires control.

### `POST /api/firewall/policy`

```json
{ "chain": "input", "policy": "deny" }
```

`200 {"message":"set input policy to deny"}`. `policy` is `allow`/`accept` or
`deny`/`drop`. Setting a chain to deny can lock the operator out; see
[firewall.md](firewall.md#changing-the-default-policy). Requires control.

## Storage

The read-only storage inventory; the contract is in [storage.md](storage.md).

### `GET /api/volumes`

```json
{
  "devices": [
    {
      "name": "sda1",
      "path": "/dev/sda1",
      "kind": "part",
      "parent": "sda",
      "size_bytes": 80396402176,
      "fstype": "ext4",
      "mountpoints": ["/"],
      "read_only": false,
      "removable": false
    }
  ],
  "mounts": [
    {
      "mountpoint": "/",
      "source": "/dev/sda1",
      "fstype": "ext4",
      "options": "rw,relatime",
      "read_only": false,
      "virtual_fs": false,
      "usage": {
        "total_bytes": 80396402176,
        "used_bytes": 37537841152,
        "available_bytes": 38247329792
      }
    }
  ],
  "swap_devices": ["/dev/sda2"],
  "control_enabled": false,
  "partition_enabled": false,
  "format_enabled": false,
  "mount_root": "/mnt",
  "disk_allow": [],
  "timestamp_ms": 1790000000000
}
```

The inventory always works; `control_enabled`, `partition_enabled` and
`format_enabled` report `[volumes]` and do not affect the read. A missing
`lsblk`, mountinfo or swaps file degrades the snapshot and adds `warning`; it
never drops the other source silently. `usage` is best-effort per mount.

### `POST /api/volumes/mount`

```json
{
  "device": "/dev/sdb1",
  "target": "/mnt/data",
  "fs_type": "ext4",
  "read_only": false,
  "allow_removable": false
}
```

`200 {"message":"mounted /dev/sdb1 on /mnt/data as ext4"}`. Requires
`[volumes] control`. The target must be an existing directory under
`mount_root`; the device must be known and unmounted.

### `POST /api/volumes/unmount`

```json
{ "target": "/mnt/data" }
```

`200 {"message":"unmounted /mnt/data"}`. Requires `[volumes] control`; refuses
`/`, virtual filesystems and paths containing panel data. A busy target is
`409 mount_busy`; permission is `403 mount_permission`; a missing target is
`404 mount_not_found`.

### Partition and format

```json
{ "disk": "/dev/sdb", "kind": "linux", "size_bytes": 1073741824 }
{ "disk": "/dev/sdb", "number": 1, "size_bytes": 2147483648 }
{ "disk": "/dev/sdb", "number": 1 }
{ "partition": "/dev/sdb1", "fs_type": "ext4", "label": "data" }
```

Routes: `POST /api/volumes/partition/create`, `/resize`, `/delete` and
`/api/volumes/format`. Create/resize/delete require `[volumes] partition` and a
whole disk in `disk_allow`; format requires `[volumes] format`. Sizes are bytes
and multiples of 512. All answer `200 {"message":"..."}`. Refusals include
`403 protected_disk`, `409 disk_busy`, `404 partition_not_found`, `400
invalid_partition`, `403 partition_permission` and `501 partition_unavailable`.

## Accounts

`GET /api/accounts` returns the users and groups snapshot described in
[accounts.md](accounts.md). It requires a live panel session unless auth is
disabled.

| Route | Body | Policy |
| --- | --- | --- |
| `POST /api/accounts/users` | `{"name":"alice","uid":1001,"gecos":"Alice","shell":"/bin/bash"}` | `[accounts] control` |
| `POST /api/accounts/users/delete` | `{"name":"alice","remove_home":false}` | `[accounts] control` |
| `POST /api/accounts/users/password` | `{"name":"alice","password":"..."}` | `[accounts] manage_passwords` |
| `POST /api/accounts/groups` | `{"name":"builders","gid":1500}` | `[accounts] control` |
| `POST /api/accounts/groups/delete` | `{"name":"builders"}` | `[accounts] control` |

All mutations answer `200 {"message":"..."}`. Refusals include `403
accounts_disabled`, `403 account_passwords_disabled`, `403 account_protected`,
`403 account_permission`, `404 account_not_found`, `409 account_conflict`, `400
invalid_account`, `501 account_unsupported` and `500 account_failed`. The
password travels in the request body and is passed to `chpasswd` on stdin; it is
never logged or placed in argv.

## Apps

App management is local state: the bundle root and `apps.d/` grants. These
routes require a live session unless auth is disabled; mutations check `Origin`
against `Host` when the header is present.

| Route | Body | Policy |
| --- | --- | --- |
| `GET /api/apps` | — | catalogue plus `enabled` and `install_from_web` |
| `POST /api/apps/install` | `{"bundle_base64":"...","accept_capabilities":false}` | `[apps] install_from_web` |
| `POST /api/apps/{id}/enable` | `{}` | — |
| `POST /api/apps/{id}/disable` | `{}` | — |
| `POST /api/apps/{id}/remove` | `{}` | — |
| `POST /api/apps/{id}/approve` | `{"accept_capabilities":true}` | — |
| `GET|POST|PUT|PATCH|DELETE /api/apps/{id}/{*path}` | raw body, max 1 MiB | method/path declared in the grant |
| `GET /api/apps/{id}/web/{*path}` | — | declared UI module |
| `GET /e/{id}/{*module}` | — | host-owned iframe bootstrap for a declared module |

Install answers `{id,name,version,capabilities,warning?}`. A signed bundle needs
`[apps] public_key`; an unsigned one is accepted only when
`require_signature = false` and carries a warning. A manifest whose capability
surface changed is `409` with code `app_capabilities_changed` until approved.
An anchor runs the guest's `anth_http` entry and maps its
`{"status":...,"body":...}` reply. A UI module is served only when the grant
declares it, with an explicit content type, `nosniff` and `no-store`. See
[apps.md](apps.md).

## Settings and lifecycle

All four routes require a live access token unless auth is disabled. The two
mutations check `Origin` against `Host`.

### `GET /api/settings`

Returns the operator policy as it is in `config.toml` (the field names match the
file). It contains no secrets: credentials live in `secrets.toml`.

### `PATCH /api/settings`

```json
{ "key": "auth.username", "value": "operator" }
```

Changes one dotted key through the same comment-preserving editor as
`config set`, validates the whole file, and writes it atomically. The daemon
keeps its current policy until a restart.

- `200 {"ok":true,"key":"auth.username","restart_required":true}`
- `400` for an unknown key, a value of the wrong type, or a file that would no
  longer parse.

### `POST /api/restart`

```json
{ "restarting": true, "pid": 1234 }
```

`202 Accepted`. The daemon answers first, then replaces itself with a fresh copy
of the same command line, which re-reads `config.toml`. The pid is kept.

### `POST /api/reboot`

```json
{ "delay_s": 0 }
```

`202 {"rebooting":true,"delay_s":0}`. The delay is at most 300 seconds. The
daemon refuses with `403 forbidden` unless it was started with `--allow-reboot`.

## Files

The routes require a live access token unless auth is disabled. Paths are
relative to `[files] root`; `..` and paths that escape through a symlink are
refused.

### `GET /api/files?path=/&limit=100&cursor=...`

```json
{
  "items": [
    { "name": "etc", "kind": "dir", "size": 0, "modified_ms": 1790000000000 },
    { "name": "hosts", "kind": "file", "size": 220, "modified_ms": 1790000000000 }
  ],
  "next": "f:hosts"
}
```

`kind` is `file`, `dir`, `symlink` or `other`. `next` is absent on the last
page. The listing puts every directory first and then the remaining entries,
each group sorted by name. `cursor` is the previous `next` value and is opaque.

### `GET /api/files/read?path=etc/hosts&offset=0&max_bytes=...`

```json
{
  "path": "etc/hosts",
  "offset": 0,
  "size": 220,
  "eof": true,
  "text": "127.0.0.1 localhost\n"
}
```

Only regular UTF-8 text files are accepted. `max_bytes` is capped by
`files.max_preview_bytes`; `eof` tells the caller whether another chunk exists.

### `GET /api/files/image?path=img/logo.png`

Returns the raw bytes for the image preview. Supported types are `png`, `jpg`,
`jpeg`, `gif`, `webp`, `bmp`, `avif`, `ico` and `svg`; the response carries the
matching `Content-Type`, `Cache-Control: no-store`, `X-Content-Type-Options:
nosniff` and a sandboxing CSP. The size is capped at the smaller of
`files.max_upload_bytes` and 32 MiB, and the same root containment rules apply.
This is an HTTP-only binary route; the socket remains JSON.

### `GET /api/files/download?path=etc/hosts`

Returns the raw bytes of any regular file as `application/octet-stream` with
`Content-Disposition: attachment`, `Cache-Control: no-store` and `nosniff`. The
same root containment rules apply and the size is capped by
`files.max_upload_bytes`. The web UI uses it for the transfer queue and names
the saved file after the request path.

### `DELETE /api/files?path=etc/hosts`

Deletes one regular file or a symlink (the link itself, not its target). The
daemon answers with 200:

```json
{ "path": "etc/hosts" }
```

Deletion requires `files.write = true`; otherwise the daemon answers `403
forbidden` with code `write_disabled`. Directories are `400 invalid`, and the
file root cannot be deleted. This is always a permanent delete: the trash is a
separate route, so a caller that wants a reversible delete must ask for it.

### `POST /api/files/trash`

```json
{ "path": "etc/hosts" }
```

Moves one file, symlink or directory to the freedesktop.org trash of its own
volume and answers:

```json
{ "path": "etc/hosts", "id": "hosts" }
```

Requires both `files.write` and `files.trash`. With the trash off the daemon
answers `403` with code `trash_disabled`; a volume with no writable trash
directory answers `403` with `trash_unavailable` instead of silently deleting
permanently. The home volume uses `$XDG_DATA_HOME/Trash`; any other volume uses
`<topdir>/.Trash/$uid` when it is a safe shared trash, otherwise
`<topdir>/.Trash-$uid`. Entries keep their original name with a numbered suffix
on collision.

### `GET /api/files/trash/list`

Returns every entry the daemon can see:

```json
{
  "entries": [
    {
      "id": "hosts",
      "name": "hosts",
      "original_path": "/etc/hosts",
      "deleted_at": "2026-10-27T13:03:01Z",
      "kind": "file",
      "size": 220,
      "location": "/root/.local/share/Trash",
      "stored_ms": 1790000000000
    }
  ],
  "total": 1,
  "truncated": false,
  "enabled": true,
  "used_bytes": 220,
  "max_bytes": 1073741824
}
```

`enabled` tells the web UI whether Delete should move to the trash. A malformed
`.trashinfo` written by another desktop is skipped, not guessed at. The listing
is capped by `files.max_entries`; `max_bytes` is absent when retention is off.

### `POST /api/files/trash/restore`

```json
{ "id": "hosts", "overwrite": false }
```

Restores one entry to its recorded path and answers `{ "id": "...", "path":
"..." }`. `Path=` comes from whichever application deleted the entry, so it is
percent-decoded, normalized without `..` and resolved inside `files.root`
again. An existing target is refused with `409` (code `exists`) unless
`overwrite` is true; `overwrite` never replaces a directory. The `.trashinfo`
is removed only after the move succeeds, so a failed restore stays retryable.

### `POST /api/files/trash/empty`

Deletes every trashed entry and answers `{ "removed": 3 }`. Requires
`files.write` and `files.trash`.

### `POST /api/files/archive`

```json
{ "path": "var/log", "destination": "var/log.tar.gz", "format": "tar.gz" }
```

Compresses one entry and answers `{ "path": "var/log.tar.gz" }`. Both fields are
optional: the destination defaults beside the entry and the format defaults to
`tar.gz`. Supported creation formats are `zip`, `tar`, `tar.gz` and `tar.xz`;
creating a `7z` is refused with `501 unsupported` (code `archive_format`). The
source is archived under its own name, so `var/log` becomes `log/...` inside the
archive. The archive is written in-process, never through a shell utility, and
refuses to overwrite an existing destination.

### `GET /api/files/archive/list?path=var/log.tar.gz`

Returns the members without writing anything:

```json
{
  "path": "var/log.tar.gz",
  "format": "tar.gz",
  "entries": [
    { "name": "log/", "kind": "dir", "size": 0 },
    { "name": "log/auth.log", "kind": "file", "size": 12 }
  ],
  "total": 2,
  "truncated": false
}
```

The format comes from the name and then from the leading bytes (`PK`, `7z`,
`\xfd7zXZ`, `\x1f\x8b`, `ustar`). Listing stops at `files.max_entries` and sets
`truncated` plus `warning` when it does.

### `POST /api/files/archive/extract`

```json
{ "path": "var/log.tar.gz", "into": "var/log-restored" }
```

Extracts an archive and answers `{ "path": "...", "entries": ["log/..."] }`.
`into` defaults to the archive name with its format suffix removed.
`files.max_archive_bytes` caps the total uncompressed size and each member, and
`files.max_entries` caps the member count; the limits are checked while writing,
not after. Absolute members, `..`, Windows drive letters, backslashes, NUL bytes
and writes through an archive-created symlink are refused, and a refused member
fails the whole request. A single wrapper directory is unwrapped when the
destination already has that name.

The supported formats follow the v0.1 contract: `zip`, `tar`, `tar.gz`/`tgz`,
`tar.xz`/`txz` and `7z` for reading; the first four for writing. `tar.xz` is
decoded as a stream and its block checks are verified; BCJ and delta filters are
refused.

### `POST /api/files/write`

```json
{ "path": "etc/motd", "content": "hello\n", "create": false }
```

Writes one UTF-8 text file and answers:

```json
{ "path": "etc/motd", "size": 6 }
```

`create: true` fails with `409 conflict` (code `exists`) instead of replacing an
existing file; the default replaces it. Writing requires `files.write = true`
(`403` with `write_disabled` otherwise). The content is capped by
`files.max_upload_bytes`; directories and non-regular files are refused, and
symlinks that leave the root are rejected. The request body limit follows
`files.max_upload_bytes` plus room for the JSON envelope.

### `POST /api/files/upload`

```json
{ "path": "img/logo.png", "content_base64": "iVBORw0KGgo=", "create": true }
```

Writes arbitrary bytes decoded from base64 and answers like the text write:

```json
{ "path": "img/logo.png", "size": 8 }
```

It has the same `files.write` gate, `create`/`exists` behavior, size cap and
path guards as the text write. The body limit allows for the base64 expansion
of `files.max_upload_bytes`.

## Terminal

`GET /api/terminal?cols=80&rows=24` upgrades to a WebSocket. It requires a live
panel session unless auth is disabled, and the `Origin` header must match
`Host`. `cols` and `rows` are optional together; omitted means 80×24, and a
single one is `400`. Sizes outside 1..500 are `400`.

The browser and server exchange JSON text frames; bytes are base64:

```jsonc
// browser -> server
{ "type": "input", "data": "<base64>" }
{ "type": "resize", "cols": 120, "rows": 40 }
{ "type": "ping" }

// server -> browser
{ "type": "ready", "shell": "/bin/sh", "cols": 80, "rows": 24, "line_editing": false }
{ "type": "output", "data": "<base64>" }
{ "type": "exit", "code": 0 }
{ "type": "error", "detail": "the terminal is disabled; set `[terminal] enabled = true`" }
{ "type": "pong" }
```

`ready` is sent before any output. An exit carries the remote code when the
platform reports it. The WebSocket frame cap is 128 KiB. One socket owns one
PTY; closing it kills the process group. `terminal_disabled`,
`terminal_capacity` and invalid-size errors are also delivered as an `error`
frame when they happen after the upgrade.

## Processes

All routes require a live access token unless auth is disabled. `control` and
`spawn` are independent switches; the details are in
[processes.md](processes.md).

### `GET /api/processes?limit=5&cursor=...`

One page of the table, busiest first:

```json
{
  "items": [
    {
      "pid": 42,
      "ppid": 1,
      "uid": 0,
      "user": "root",
      "name": "sleep",
      "command": "sleep 30",
      "state": "S",
      "cpu_ticks": 12,
      "rss_kib": 1024,
      "threads": 1,
      "start_ticks": 98765
    }
  ],
  "next": null,
  "total": 793,
  "clock_ticks": 100,
  "timestamp_ms": 1790000000000,
  "control_enabled": true,
  "spawn_enabled": true,
  "spawn_allow": ["/bin/sleep"]
}
```

`limit` is 1–1000 (default 100) and `next` is opaque. `pid` plus `start_ticks`
is the stable identity; the client computes CPU percent from two snapshots.

### `GET /api/processes/watch?interval=2s&limit=1000`

Server-Sent Events, the same shape as `host-info/watch`:

- `event: processes`
- `id`: `timestamp_ms`
- `data`: one snapshot (the first page)
- `retry: 3000`
- keep-alive comments every 15 seconds.

### `POST /api/processes/signal`

```json
{ "pid": 42, "start_ticks": 98765, "signal": "term", "group": false }
```

`200 {"pid":42,"signal":"term"}`. Requires `process.control`. Refusals:
`403 process_control_disabled`, `403 protected_process`, `403 process_not_owned`,
`409 process_changed`, `400 not_group_leader`.

### `POST /api/processes/spawn`

```json
{ "program": "/bin/sleep", "args": ["30"], "cwd": "/tmp" }
```

`200 {"pid":99,"program":"/bin/sleep","args":["30"]}`. Requires `process.spawn`
and a program in `spawn_allow`. Refusals: `403 process_spawn_disabled`, `403
program_not_allowed`, `403 program_not_executable`, `404 not_found`, `400
program_not_absolute`, `400 cwd_not_absolute`, `404 cwd_not_found`.

## Static UI

`GET /` and `GET /{*path}` serve the embedded shell. A missing file is `404`; an
unknown browser route falls back to `index.html`. `/api` never falls back to the
HTML shell.

## Status codes

| Kind | HTTP |
| --- | --- |
| `not_found` | 404 |
| `forbidden` | 403 |
| `unauthorized` | 401 |
| `conflict` | 409 |
| `invalid` | 400 |
| `unsupported` | 501 |
| `too_large` | 413 |
| `too_many` | 429 |
| `internal` | 500 |
