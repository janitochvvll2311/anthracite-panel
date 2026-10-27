# Socket protocol

The daemon speaks length-prefixed JSON over a Unix socket.

## Framing

Each message is:

```
u32 big-endian payload length
payload: JSON, exactly that many bytes
```

- A frame carries one `Request` or one `Response`.
- The limit is 64 MiB. A larger frame is rejected with `too_large`; it is never
  shortened.
- The first frame on a connection **must** be `Hello`. A different first frame
  receives an `invalid_hello` error and the connection closes.

## Handshake

```json
{
  "type": "hello",
  "data": {
    "protocol_version": 2,
    "client": "cli/0.3.0",
    "features": []
  }
}
```

The daemon answers:

```json
{
  "type": "welcome",
  "data": {
    "protocol_version": 2,
    "server": "anth-panel",
    "version": "0.3.0",
    "features": []
  }
}
```

- `protocol_version` must match. A mismatch is `invalid` with code
  `protocol_mismatch` and closes the connection.
- `client` is a printable label of at most 64 characters.
- `features` is reserved for post-1.0 negotiation. It is empty today; unknown
  features are ignored.

## Requests in this phase

```json
{ "type": "host_info" }
```

The answer is:

```json
{
  "type": "host_info",
  "data": {
    "hostname": "example",
    "os": {
      "name": "Debian GNU/Linux",
      "version": "13",
      "kernel": "6.12.0",
      "arch": "x86_64"
    },
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
}
```

Unreadable host fields degrade to `unknown` or `0`; the request still succeeds.
`collected_at_ms` is Unix epoch milliseconds and is also the SSE event id.

### Lifecycle

```json
{ "type": "restart" }
```

```json
{ "type": "restarting", "data": { "pid": 1234 } }
```

`restart` replaces the process with a fresh copy of the same command line after
the answer is written. The pid is kept and the new image re-reads the config.

```json
{ "type": "system_reboot", "data": { "delay_s": 0 } }
```

```json
{ "type": "rebooting", "data": { "delay_s": 0 } }
```

`delay_s` is at most 300. The daemon refuses with `forbidden` unless it was
started with `--allow-reboot`. Both actions answer before they run.

### Files

```json
{ "type": "files_list", "data": { "path": "/etc", "limit": 100, "cursor": null } }
```

```json
{
  "type": "files_list",
  "data": {
    "items": [
      { "name": "hosts", "kind": "file", "size": 220, "modified_ms": 1790000000000 }
    ],
    "next": "f:hosts"
  }
}
```

```json
{ "type": "files_read", "data": { "path": "etc/hosts", "offset": 0, "max_bytes": 8192 } }
```

```json
{
  "type": "files_read",
  "data": {
    "path": "etc/hosts",
    "offset": 0,
    "size": 220,
    "eof": true,
    "text": "127.0.0.1 localhost\n"
  }
}
```

```json
{ "type": "files_delete", "data": { "path": "etc/hosts" } }
```

```json
{ "type": "files_deleted", "data": { "path": "etc/hosts" } }
```

```json
{
  "type": "files_write",
  "data": { "path": "etc/motd", "content": "hello\n", "create": true }
}
```

```json
{ "type": "files_written", "data": { "path": "etc/motd", "size": 6 } }
```

```json
{
  "type": "files_upload",
  "data": { "path": "img/logo.png", "content_base64": "iVBORw0KGgo=", "create": true }
}
```

Paths are relative to the configured file root. `..`, absolute names and
symlinks that leave the root are refused with `forbidden`; a non-UTF-8 file is
`unsupported`. `files_delete` removes one regular file or a symlink (the link,
not its target), refuses directories and requires `files.write`; with writes
disabled it answers `forbidden` and code `write_disabled`. `files_write` writes
one UTF-8 text file, caps the content at `files.max_upload_bytes`, requires
`files.write`, and answers `conflict` with code `exists` when `create` is true
and the file is already there. `files_upload` does the same for arbitrary bytes
carried as base64 in `content_base64`, and answers with the same
`files_written`.

```json
{ "type": "files_trash", "data": { "path": "etc/hosts" } }
```

```json
{ "type": "files_trashed", "data": { "path": "etc/hosts", "id": "hosts" } }
```

```json
{ "type": "files_trash_list" }
```

```json
{
  "type": "files_trash_list",
  "data": {
    "entries": [
      {
        "id": "hosts",
        "name": "hosts",
        "original_path": "/etc/hosts",
        "deleted_at": "2026-10-27T13:03:01Z",
        "kind": "file",
        "size": 220,
        "location": "/root/.local/share/Trash"
      }
    ],
    "total": 1,
    "truncated": false,
    "enabled": true,
    "used_bytes": 220,
    "max_bytes": 1073741824
  }
}
```

```json
{ "type": "files_trash_restore", "data": { "id": "hosts", "overwrite": false } }
```

```json
{ "type": "files_restored", "data": { "id": "hosts", "path": "etc/hosts" } }
```

```json
{ "type": "files_trash_empty" }
```

```json
{ "type": "files_trash_emptied", "data": { "removed": 1 } }
```

`files_trash` moves one entry to the freedesktop.org trash of its own volume and
needs both `files.write` and `files.trash`; a disabled trash answers `forbidden`
with code `trash_disabled`, and a volume without a writable trash directory
answers `trash_unavailable` instead of deleting permanently. `files_trash_list`
reports the entries any desktop would see, skipping malformed `.trashinfo`
files. `files_trash_restore` re-validates the recorded `Path=` against the
configured root; an existing target is `conflict`/`exists` unless `overwrite` is
true, and `overwrite` never replaces a directory.

```json
{ "type": "files_archive", "data": { "path": "var/log", "format": "tar.gz", "destination": null } }
```

```json
{ "type": "files_archive_created", "data": { "path": "var/log.tar.gz" } }
```

```json
{ "type": "files_archive_list", "data": { "path": "var/log.tar.gz" } }
```

```json
{
  "type": "files_archive_list",
  "data": {
    "path": "var/log.tar.gz",
    "format": "tar.gz",
    "entries": [
      { "name": "log/", "kind": "dir", "size": 0 },
      { "name": "log/auth.log", "kind": "file", "size": 12 }
    ],
    "total": 2,
    "truncated": false
  }
}
```

```json
{ "type": "files_archive_extract", "data": { "path": "var/log.tar.gz", "into": null } }
```

```json
{ "type": "files_archive_extracted", "data": { "path": "var/log", "entries": ["log/", "log/auth.log"] } }
```

Archives are handled in-process. `zip`, `tar`, `tar.gz` and `tar.xz` can be
created and read; `7z` is read-only. Extraction refuses absolute members, `..`,
Windows drive letters, backslashes, NUL bytes and writes through an
archive-created symlink, and it enforces `files.max_archive_bytes` while
writing. A refused member fails the request instead of leaving a partial tree
that looks complete.

### Terminal

```json
{ "type": "terminal_open", "data": { "cols": 80, "rows": 24 } }
```

```json
{
  "type": "terminal_opened",
  "data": {
    "id": "t1a",
    "program": "/bin/sh",
    "cols": 80,
    "rows": 24,
    "line_editing": false
  }
}
```

```json
{ "type": "terminal_read", "data": { "id": "t1a", "wait_ms": 10000 } }
```

```json
{
  "type": "terminal_output",
  "data": { "id": "t1a", "seq": 1, "bytes_base64": "aGkK", "eof": false }
}
```

```json
{ "type": "terminal_write", "data": { "id": "t1a", "bytes_base64": "bHMK" } }
{ "type": "terminal_resize", "data": { "id": "t1a", "cols": 120, "rows": 40 } }
{ "type": "terminal_close", "data": { "id": "t1a" } }
```

A write, resize or close answers `{"type":"accepted"}`. A read waits at most
`wait_ms` (capped at 30 seconds); an empty answer means the wait expired. `seq`
starts at zero and advances only when bytes are returned. `eof: true` marks the
end and carries `exit_code` when available. Sizes are 1..500 columns/rows;
output chunks are at most 16 KiB and input is capped at 16 KiB after decoding.
Handles are owner-scoped: a handle from another auth session or socket uid is
`not_found`. With `[terminal] enabled = false`, every terminal request answers
`forbidden` with code `terminal_disabled`.

### Network

```json
{ "type": "network" }
```

```json
{
  "type": "network",
  "data": {
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
}
```

Counters are cumulative; a client derives a rate from two snapshots. The
address `scope` is `host`, `link` or `global`. `speed_mbps` is omitted when the
driver reports no positive speed. See [network.md](network.md).

Mutations use the same dispatch as HTTP:

```json
{ "type": "network_set_state", "data": { "name": "eth0", "up": false } }
{ "type": "network_create", "data": { "name": "br0", "kind": "bridge", "mtu": 1500 } }
{ "type": "network_edit", "data": { "name": "eth0", "mtu": 1400 } }
{ "type": "network_rename", "data": { "name": "eth0", "new_name": "lan0" } }
{ "type": "network_delete", "data": { "name": "eth0" } }
```

Each answers `{"type":"network_changed","data":{"message":"..."}}`. A socket
request has no peer address, so the delete path guard is skipped; loopback and
last-default-route guards still apply. `[network] control` and
`[network] manage` are independent.

### Firewall

```json
{ "type": "firewall_status" }
{ "type": "firewall_add", "data": { "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" } }
{ "type": "firewall_delete", "data": { "handle": 4 } }
{ "type": "firewall_flush" }
```

Status answers:

```json
{
  "type": "firewall_snapshot",
  "data": {
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
    "ruleset": "table inet anthracite { ... }",
    "timestamp_ms": 1790000000000
  }
}
```

Every mutation answers `{"type":"firewall_changed","data":{"message":"..."}}`.
The managed table is `inet anthracite`, its only chain is `input`, and every
operation requires `[firewall] control`. See [firewall.md](firewall.md).

### Processes

```json
{ "type": "processes", "data": { "limit": 100, "cursor": null } }
```

```json
{
  "type": "processes",
  "data": {
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
}
```

`limit` is 1–1000; `next` is an opaque offset into the sorted snapshot. `pid`
alone is not an identity: a mutation carries `start_ticks` and a mismatch is
`conflict` / `process_changed`.

```json
{
  "type": "process_signal",
  "data": { "pid": 42, "start_ticks": 98765, "signal": "term", "group": false }
}
```

```json
{ "type": "process_signalled", "data": { "pid": 42, "signal": "term" } }
```

Signals are `term`, `kill`, `hup`, `int`, `quit`, `stop` and `cont`; `group:
true` signals the process group led by `pid`. Requires `[process] control`.

```json
{
  "type": "process_spawn",
  "data": { "program": "/bin/sleep", "args": ["30"], "cwd": null }
}
```

```json
{
  "type": "process_started",
  "data": { "pid": 99, "program": "/bin/sleep", "args": ["30"] }
}
```

The program must be absolute and appear in `[process] spawn_allow`; it runs
detached with null stdio and is reaped by a thread. Nothing goes through a
shell. Requires `[process] spawn`.

## Errors

```json
{
  "type": "error",
  "data": {
    "kind": "invalid",
    "code": "protocol_mismatch",
    "message": "this daemon speaks protocol 2, the client speaks 1"
  }
}
```

`kind` is one of `not_found`, `forbidden`, `unauthorized`, `conflict`, `invalid`,
`unsupported`, `too_large`, `too_many`, `internal`. `code` is the stable machine
tag; `message` is for a human.

## Pagination

Lists will use:

```json
{
  "limit": 100,
  "cursor": null
}
```

and answer with `items`, `next` and optional `total`. `next = null` is the only
end signal. A cursor is opaque; the Files listing uses it.

## Peer credentials

The listener checks `SO_PEERCRED`. A connection is accepted from the daemon's
own uid or root. The socket is created mode `0660`. A caller that is neither the
daemon user nor root must use the authenticated HTTP API instead.

## Versioning

`PROTOCOL_VERSION` is 2 for the v0.3 line. Before 1.0.0 a breaking change bumps
the version and is recorded in [protocol-changelog](design/protocol-changelog.md);
there is no compatibility promise yet.
