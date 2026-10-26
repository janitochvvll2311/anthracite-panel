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
    "next": "hosts"
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

Paths are relative to the configured file root. `..`, absolute names and
symlinks that leave the root are refused with `forbidden`; a non-UTF-8 file is
`unsupported`.

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
end signal. A cursor is opaque. No list uses it yet.

## Peer credentials

The listener checks `SO_PEERCRED`. A connection is accepted from the daemon's
own uid or root. The socket is created mode `0660`. A caller that is neither the
daemon user nor root must use the authenticated HTTP API instead.

## Versioning

`PROTOCOL_VERSION` is 2 for the v0.3 line. Before 1.0.0 a breaking change bumps
the version and is recorded in [protocol-changelog](design/protocol-changelog.md);
there is no compatibility promise yet.
