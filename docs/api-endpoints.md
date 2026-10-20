# HTTP API

The API is served by `anth-panel api` or `anth-panel run` and proxies each
command to the daemon over its Unix socket.

## Conventions

**The route names the command.** There is no `type` member and no `data`
wrapper: arguments travel as top-level members of the JSON body. The socket
envelope documented in [socket-protocol.md](socket-protocol.md) stays a
socket-only concern.

```bash
# socket transport
{"type":"Echo","data":{"message":"hola"}}

# HTTP transport
POST /api/echo
{"message":"hola"}
```

**Paths mirror the CLI.** A subcommand becomes a path segment; a flag that turns
it into a stream becomes `/watch`.

| CLI | HTTP |
| --- | --- |
| `anth-panel host-info` | `GET /api/host-info` |
| `anth-panel host-info --watch 2s` | `GET /api/host-info/watch?interval=2s` |
| `anth-panel echo hola` | `POST /api/echo` with `{"message":"hola"}` |
| `anth-panel process list` | `GET /api/processes` |
| `anth-panel process list --watch 2s` | `GET /api/processes/watch?interval=2s` |
| `anth-panel process signal 123 term` | `POST /api/processes/123/signal` with `{"signal":"term"}` |
| `anth-panel network list` | `GET /api/network` |
| `anth-panel network list --watch 2s` | `GET /api/network/watch?interval=2s` |
| `anth-panel network down eth0` | `POST /api/network/eth0/state` with `{"up":false}` |
| `anth-panel firewall status` | `GET /api/firewall` |
| `anth-panel firewall allow 22/tcp` | `POST /api/firewall/rules` with `{"action":"allow","port":22}` |
| `anth-panel firewall delete 3` | `POST /api/firewall/rules/3/delete` |
| `anth-panel firewall flush` | `POST /api/firewall/flush` |
| `anth-panel volume list` | `GET /api/volumes` |
| `anth-panel volume mount /dev/sdb1 /mnt/data` | `POST /api/volumes/mount` with `{"device":"/dev/sdb1","target":"/mnt/data"}` |
| `anth-panel volume unmount /mnt/data` | `POST /api/volumes/unmount` with `{"target":"/mnt/data"}` |
| `anth-panel file list /tmp` | `GET /api/files?path=/tmp` |
| `anth-panel file read /tmp/a` | `GET /api/files/content?path=/tmp/a` |
| `anth-panel file mkdir /tmp/a` | `POST /api/files/mkdir` with `{"path":"/tmp/a"}` |
| `anth-panel file rename /a /b` | `POST /api/files/rename` with `{"from":"/a","to":"/b"}` |
| `anth-panel file delete /a` | `POST /api/files/delete` with `{"path":"/a"}` |
| `anth-panel file upload ./a /b` | `POST /api/files/upload?path=/b` with a raw body |

**HTTP errors are always JSON.** Every HTTP response that fails, including a
malformed body rejected before the handler runs, has this shape:

```json
{"error": "invalid interval `abc`: add a unit suffix (ms, s, m or h), e.g. 500ms, 2s, 1m"}
```

After a WebSocket upgrade there is no HTTP status left to change; terminal
failures are `error` frames instead. See `GET /api/terminal` below.

## Routes

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/healthz` | Liveness probe |
| `GET` | `/api/host-info` | One host snapshot |
| `GET` | `/api/host-info/watch` | Host snapshots as SSE |
| `POST` | `/api/echo` | Round-trip a message |
| `GET` | `/api/terminal` | Interactive PTY over WebSocket |
| `GET` | `/api/processes` | One process-table snapshot |
| `GET` | `/api/processes/watch` | Process snapshots as SSE |
| `POST` | `/api/processes/{pid}/signal` | Send an allowlisted signal |
| `GET` | `/api/network` | One network-interface snapshot |
| `GET` | `/api/network/watch` | Network snapshots as SSE |
| `POST` | `/api/network/{name}/state` | Bring an interface up or down |
| `GET` | `/api/firewall` | One managed firewall snapshot |
| `POST` | `/api/firewall/rules` | Add a deny or remove a managed deny |
| `POST` | `/api/firewall/rules/{handle}/delete` | Delete a rule by handle |
| `POST` | `/api/firewall/flush` | Flush the managed chain |
| `GET` | `/api/volumes` | One storage-inventory snapshot |
| `POST` | `/api/volumes/mount` | Mount a device on a directory |
| `POST` | `/api/volumes/unmount` | Unmount a mounted directory |
| `GET` | `/api/files` | One directory listing |
| `GET` | `/api/files/content` | Inline file preview |
| `GET` | `/api/files/download` | File download |
| `POST` | `/api/files/mkdir` | Create a directory |
| `POST` | `/api/files/rename` | Rename an entry |
| `POST` | `/api/files/delete` | Delete an entry |
| `POST` | `/api/files/upload` | Upload a raw body |

Anything else falls through to the embedded web UI, which serves static files
and returns `index.html` for unknown paths so the SPA can route.

## `GET /healthz`

Returns `200` with the body `ok`. It does not touch the daemon, so it answers
even when the daemon is down — it reports that the HTTP server is alive, not
that the system is healthy end to end.

```bash
$ curl -s localhost:8080/healthz
ok
```

## `GET /api/host-info`

One snapshot of the host the daemon runs on.

```bash
curl -s localhost:8080/api/host-info
```

```json
{
  "hostname": "box",
  "os": {
    "name": "Debian GNU/Linux",
    "version": "13",
    "kernel": "6.12.111+deb13-amd64",
    "arch": "x86_64"
  },
  "uptime_seconds": 563487,
  "cpu_model": "Intel Core Processor (Haswell, no TSX)",
  "logical_cores": 4,
  "load": { "one": 0.99, "five": 0.52, "fifteen": 0.38 },
  "memory": {
    "total_kib": 7943448,
    "available_kib": 4574196,
    "swap_total_kib": 0,
    "swap_free_kib": 0
  }
}
```

| Status | When |
| --- | --- |
| `200` | Snapshot returned |
| `502` | The daemon answered with an error |
| `503` | No daemon is listening on the socket |

Field meanings and units are the same as in
[socket-protocol.md](socket-protocol.md#hostinfo-reply).

## `GET /api/host-info/watch`

A snapshot per interval, streamed with
[Server-Sent Events](https://html.spec.whatwg.org/multipage/server-sent-events.html).

```bash
curl -N "localhost:8080/api/host-info/watch?interval=2s"
```

### Query parameters

| Parameter | Default | Description |
| --- | --- | --- |
| `interval` | `2s` | Time between snapshots |

The interval takes an `ms`, `s`, `m` or `h` suffix and may be fractional
(`1.5s`). It is rejected below `10ms` and above `24h`, with the same parser the
CLI uses.

### Response

```
HTTP/1.1 200 OK
content-type: text/event-stream
cache-control: no-cache
transfer-encoding: chunked
```

```
retry: 3000

event: host-info
data: {"hostname":"box","os":{...},"uptime_seconds":563487,...}
id: 1

event: host-info
data: {"hostname":"box",...}
id: 2
```

| Field | Meaning |
| --- | --- |
| `retry: 3000` | Sent once, before the first event. Tells `EventSource` to reconnect after 3s. |
| `event:` | Always `host-info` for this route, or `error` if the daemon fails mid-stream. |
| `data:` | The same JSON object `GET /api/host-info` returns. |
| `id:` | Increments per snapshot, starting at `1`. `error` uses `error`. |

A keep-alive comment is sent after 15s of silence so an idle connection is not
closed by an intermediary. This matters when `interval` is long.

### Errors

Before the first event the status is still writable:

| Status | When |
| --- | --- |
| `400` | `interval` is malformed, below `10ms`, above `24h`, or repeated |
| `503` | No daemon is listening |

A repeated parameter (`?interval=1s&interval=2s`) is also a `400` in the usual
JSON shape, not axum's text/plain rejection.

After streaming starts the status is already `200`, so a failure is reported as
an event instead, and then the stream ends:

```
event: error
id: error
data: send failed: Broken pipe (os error 32)
```

A client should treat the `error` event as terminal and reconnect with
`EventSource`, which honours the `retry` hint.

### Reading it from a browser

```js
const source = new EventSource("/api/host-info/watch?interval=2s");

source.addEventListener("host-info", (event) => {
  const info = JSON.parse(event.data);
  console.log(info.hostname, info.memory.available_kib);
});

source.addEventListener("error", () => {
  // Either a server-sent error event or a dropped connection; EventSource
  // reconnects on its own after the retry hint.
});
```

## `GET /api/processes`

One snapshot of the process table on the daemon host. The daemon reads `/proc`
and resolves uid names from `/etc/passwd`.

```bash
curl -s localhost:8080/api/processes
```

```json
{
  "processes": [
    {
      "pid": 1234,
      "ppid": 1,
      "uid": 1000,
      "user": "alice",
      "name": "sleep",
      "command": "sleep 30",
      "state": "S",
      "cpu_ticks": 12,
      "rss_kib": 2048,
      "threads": 1,
      "start_ticks": 12345
    }
  ],
  "clock_ticks": 100,
  "timestamp_ms": 1700000000000,
  "control_enabled": false,
  "total": 1,
  "truncated": false
}
```

| Field | Meaning |
| --- | --- |
| `clock_ticks` | `sysconf(_SC_CLK_TCK)`, for converting tick deltas to CPU percent |
| `timestamp_ms` | Wall-clock time of the snapshot |
| `control_enabled` | Whether `[process] control` allows signals |
| `total` | Processes found before truncation |
| `truncated` | Whether rows or command lines were shortened |
| `warning` | Present only when rows were dropped |

A snapshot returns at most 2000 processes and truncates each command line to
256 characters. Processes are sorted by accumulated CPU before truncation. The
serialized snapshot is then fitted to the 1 MiB protocol line cap by shortening
commands further and, if necessary, dropping the least busy rows. Command
shortening sets `truncated` without a warning. When rows are dropped,
`truncated` is `true` and `warning` contains a human-readable message such as:

```json
{"warning":"showing 2000 of 4312 processes, highest CPU first"}
```

CPU percent is **not** computed by the server. Clients calculate it from two
snapshots with `cpu_ticks`, `clock_ticks` and `timestamp_ms`.

| Status | When |
| --- | --- |
| `200` | Snapshot returned |
| `502` | The daemon answered with an error |
| `503` | No daemon is listening on the socket |

See [processes.md](processes.md) for the signal policy and truncation rules.

## `GET /api/processes/watch`

A process snapshot per interval, streamed with Server-Sent Events. The event
name is `processes`; each `data` line is the same object `GET /api/processes`
returns.

```bash
curl -N "localhost:8080/api/processes/watch?interval=2s"
```

| Parameter | Default | Description |
| --- | --- | --- |
| `interval` | `2s` | Time between snapshots |

The interval uses the same parser as `host-info/watch`, including the `10ms`
minimum and `24h` maximum. Errors before the stream starts are `400`/`503` with
the usual JSON shape; a failure mid-stream becomes an `error` event and ends the
stream.

## `POST /api/processes/{pid}/signal`

Sends one allowlisted signal to a process.

```bash
curl -s -X POST localhost:8080/api/processes/1234/signal \
  -H 'content-type: application/json' \
  -d '{"signal":"term"}'
```

```json
{"pid":1234,"signal":"term"}
```

Allowed signals: `term`, `kill`, `hup`, `int`, `quit`, `stop`, `cont`.
Names are case-insensitive.

The route returns `403` when `[process] control = false`. Even when enabled, the
daemon refuses pid 1, its own pid, processes owned by another user, and signals
outside the allowlist. A rejected signal is a `400` with the daemon's error
detail.

| Status | When |
| --- | --- |
| `200` | Signal delivered |
| `400` | Invalid body/signal, or the daemon rejected the target |
| `403` | Process control is disabled |
| `502` | The daemon answered with an unexpected error |
| `503` | No daemon is listening on the socket |


## `GET /api/network`

One snapshot of every network interface on the daemon host.

```bash
curl -s localhost:8080/api/network
```

```json
{
  "interfaces": [
    {
      "name": "eth0",
      "index": 2,
      "state": "up",
      "up": true,
      "running": true,
      "loopback": false,
      "mac": "aa:bb:cc:dd:ee:ff",
      "mtu": 1500,
      "speed_mbps": 1000,
      "duplex": "full",
      "addresses": [
        { "family": "ipv4", "address": "192.0.2.10", "prefix_len": 24 }
      ],
      "rx_bytes": 12345,
      "tx_bytes": 67890,
      "rx_packets": 10,
      "tx_packets": 20,
      "rx_errors": 0,
      "tx_errors": 0,
      "rx_dropped": 0,
      "tx_dropped": 0
    }
  ],
  "timestamp_ms": 1700000000000,
  "control_enabled": false,
  "total": 1,
  "truncated": false
}
```

| Field | Meaning |
| --- | --- |
| `state` | Operational state from `/sys/class/net/<name>/operstate` |
| `up` | Administrative `IFF_UP` flag |
| `running` | Carrier/running `IFF_RUNNING` flag |
| `loopback` | Loopback `IFF_LOOPBACK` flag |
| `speed_mbps` | Negotiated link speed, or `null` |
| `duplex` | `full`, `half`, or `null` |
| `addresses` | IPv4/IPv6 addresses with a prefix length |
| `timestamp_ms` | Wall-clock time of the snapshot |
| `control_enabled` | Whether `[network] control` allows up/down requests |
| `total` | Interfaces found before the row cap |
| `truncated` | Whether the interface list was shortened |
| `warning` | Present only when the list was shortened |

RX/TX counters are cumulative. Clients compute rates from two snapshots with
`timestamp_ms`; the server never keeps a sampler. A snapshot returns at most
2000 interfaces and is shrunk further to fit the 1 MiB line cap; either case
sets `truncated` and includes a `warning`.

| Status | When |
| --- | --- |
| `200` | Snapshot returned |
| `502` | The daemon answered with an error |
| `503` | No daemon is listening on the socket |

See [network.md](network.md) for the data sources and policy.

## `GET /api/network/watch`

A network snapshot per interval as Server-Sent Events. The event name is
`network`; each `data` line is the same object `GET /api/network` returns.

```bash
curl -N "localhost:8080/api/network/watch?interval=2s"
```

The interval uses the same parser as `host-info/watch`, including the `10ms`
minimum and `24h` maximum. Errors before the stream starts are `400`/`503`;
a failure mid-stream becomes an `error` event and ends the stream.

## `POST /api/network/{name}/state`

Brings one interface up or down.

```bash
curl -s -X POST localhost:8080/api/network/eth0/state \
  -H 'content-type: application/json' \
  -d '{"up":false}'
```

```json
{"name":"eth0","up":false}
```

The route returns `403` when `[network] control = false`. The daemon also
refuses invalid names, unknown interfaces, and bringing a loopback interface
down. State changes
require a daemon user with `CAP_NET_ADMIN`; a permission failure is `403`.

| Status | When |
| --- | --- |
| `200` | State changed |
| `400` | Invalid name or body |
| `403` | Network control is disabled, the interface is protected, or permission was denied |
| `404` | The interface does not exist |
| `500` | The kernel operation failed |
| `503` | No daemon is listening on the socket |

## `GET /api/firewall`

One snapshot of the dedicated `inet anthracite` nftables table.

```bash
curl -s localhost:8080/api/firewall
```

```json
{
  "initialized": true,
  "control_enabled": false,
  "rules": [
    {
      "handle": 3,
      "chain": "input",
      "action": "allow",
      "protocol": "tcp",
      "port": 22,
      "comment": "ssh"
    }
  ],
  "ruleset": "table inet anthracite { ... }",
  "total": 1,
  "truncated": false
}
```

| Field | Meaning |
| --- | --- |
| `initialized` | Whether the managed table exists |
| `control_enabled` | Whether `[firewall] control` allows changes |
| `rules` | Rules in the managed `input` chain |
| `ruleset` | Textual `nft list table inet anthracite` output |
| `total` | Number of managed rules before any truncation |
| `truncated` | Whether `rules` or `ruleset` was shortened |
| `warning` | Present only when the snapshot was shortened |

A snapshot too large for the 1 MiB protocol line cap drops `ruleset` first and
then the rules with the highest handles, and adds a `warning` such as
`showing 4000 of 6000 firewall rules, lowest handle first`.

| Status | When |
| --- | --- |
| `200` | Snapshot returned |
| `403` | The daemon lacks permission to run `nft` |
| `503` | `nft` is unavailable or no daemon is listening |

## `POST /api/firewall/rules`

Adds a deny rule, or removes a matching managed deny for `allow`. `protocol`
defaults to `tcp`. The managed chain is a blocklist with an accept policy, so
`allow` cannot override another firewall table.

```bash
curl -s -X POST localhost:8080/api/firewall/rules \
  -H 'content-type: application/json' \
  -d '{"action":"deny","port":8080,"protocol":"tcp","comment":"web"}'
```

```json
{"message":"denied tcp/8080"}
```

The route returns `403` when `[firewall] control = false`. Comments are limited
to 32 characters from `A-Z`, `a-z`, `0-9`, `.`, `_` and `-`.

| Status | When |
| --- | --- |
| `200` | Rule added |
| `400` | Invalid body, port, protocol or comment |
| `403` | Firewall control is disabled or permission was denied |
| `503` | `nft` is unavailable or no daemon is listening |
| `500` | The `nft` command failed |

## `POST /api/firewall/rules/{handle}/delete`

Deletes one managed rule by the handle returned by `GET /api/firewall`.

```bash
curl -s -X POST localhost:8080/api/firewall/rules/3/delete
```

```json
{"message":"deleted firewall rule 3"}
```

A handle that does not exist is `404`.

## `POST /api/firewall/flush`

Removes every rule from `inet anthracite input`.

```bash
curl -s -X POST localhost:8080/api/firewall/flush
```

```json
{"message":"flushed 1 firewall rule"}
```

See [firewall.md](firewall.md) for the table layout, container capabilities and
security notes.

## Volumes

See [volumes.md](volumes.md) for the data sources, the mount and unmount
policy and the error kinds.

### `GET /api/volumes`

One snapshot of block devices and every mount in the daemon's mount namespace.

```bash
curl -s localhost:8080/api/volumes
```

```json
{
  "devices": [
    {
      "name": "sdb1",
      "path": "/dev/sdb1",
      "kind": "part",
      "parent": "sdb",
      "size_bytes": 16000000000,
      "fstype": "ext4",
      "label": "data",
      "uuid": "1111-2222",
      "mountpoints": [],
      "read_only": false,
      "removable": false,
      "model": "USB DISK",
      "transport": "usb"
    }
  ],
  "mounts": [
    {
      "mountpoint": "/",
      "source": "overlay",
      "fstype": "overlay",
      "options": "rw,relatime",
      "read_only": false,
      "virtual_fs": true,
      "usage": {
        "total_bytes": 80000000000,
        "used_bytes": 20000000000,
        "available_bytes": 55000000000
      }
    }
  ],
  "control_enabled": true,
  "timestamp_ms": 1700000000000,
  "total": 1,
  "truncated": false
}
```

| Field | Meaning |
| --- | --- |
| `devices` | Block devices from `lsblk`, flattened, tree order |
| `devices[].parent` | Kernel name of the parent device, absent at the root |
| `devices[].mountpoints` | Every path the device is mounted on; always present, `[]` when unmounted |
| `devices[].removable` | Whether the device reports itself as removable media |
| `mounts` | Every entry in `/proc/self/mountinfo` |
| `mounts[].virtual_fs` | `true` for `proc`, `sysfs`, `tmpfs`, `cgroup2`, overlays … |
| `mounts[].usage` | `statvfs` figures; absent when the filesystem cannot be read |
| `control_enabled` | Whether `[volumes] control` allows mount and unmount |
| `total` | Number of devices before any truncation |
| `truncated` | Whether the device or mount list was shortened |
| `warning` | Present only when the snapshot was shortened |

A snapshot too large for the 1 MiB protocol line cap drops the longest
diagnostic fields first, then the device tail and finally the mount tail, and
adds a `warning` such as `showing 4000 of 20000 devices; the snapshot was
shortened to fit the protocol line cap`.

| Status | When |
| --- | --- |
| `200` | Snapshot returned |
| `503` | No daemon is listening |

`GET /api/volumes` has **no watch variant**. Mounts change when an operator
changes them, so the web component refreshes on demand.

### `POST /api/volumes/mount`

```bash
curl -s -X POST localhost:8080/api/volumes/mount \
  -H 'content-type: application/json' \
  -d '{"device":"/dev/sdb1","target":"/mnt/data","read_only":true}'
```

| Field | Required | Meaning |
| --- | --- | --- |
| `device` | yes | Full path or kernel name of an unmounted block device |
| `target` | yes | Existing absolute directory, inside `volumes.mount_root` |
| `fs_type` | no | Filesystem type; detected from the device when omitted |
| `read_only` | no | Mount with `-o ro`, defaults to `false` |
| `allow_removable` | no | Permit removable media, defaults to `false` |

```json
{"message":"mounted /dev/sdb1 on /mnt/data as ext4"}
```

| Status | When |
| --- | --- |
| `200` | Mounted |
| `400` | Bad path, bad type, relative target or the device is already mounted |
| `403` | Control is off, the target is outside `mount_root`, or removable media without `allow_removable` |
| `403` | The command could not elevate |
| `404` | The device or target does not exist |
| `500` | `mount` failed for any other reason |

### `POST /api/volumes/unmount`

```bash
curl -s -X POST localhost:8080/api/volumes/unmount \
  -H 'content-type: application/json' \
  -d '{"target":"/mnt/data"}'
```

```json
{"message":"unmounted /mnt/data"}
```

| Status | When |
| --- | --- |
| `200` | Unmounted |
| `400` | The target is relative |
| `403` | Control is off, the target is `/`, a virtual filesystem, or an ancestor of a path the panel needs |
| `403` | The command could not elevate |
| `404` | The target is not a mountpoint |
| `409` | The filesystem is busy |
| `500` | `umount` failed for any other reason |

## Files

Filesystem paths are virtual: `/` is `[files] root` from the daemon config.
Every route rejects paths containing `..` and symlinks that escape the root.
Writes additionally require `[files] write = true`; otherwise they return `403`
before reaching the daemon. See [files.md](files.md) for the full policy.

### `GET /api/files`

Lists one directory. `path` is optional and defaults to `/`.

```bash
curl -s "localhost:8080/api/files?path=/var/log"
```

```json
{
  "path": "/var/log",
  "parent": "/var",
  "entries": [
    {
      "name": "syslog",
      "path": "/var/log/syslog",
      "kind": "file",
      "size": 1024,
      "modified_ms": 1700000000000,
      "hidden": false
    }
  ],
  "total": 1,
  "truncated": false,
  "write_enabled": false
}
```

`kind` is `file`, `directory`, `symlink` or `other`. Symlinks additionally carry
`target`. `total` is the count before `max_entries` was applied; `truncated` and
`warning` explain a shortened listing.

| Status | When |
| --- | --- |
| `200` | Listing returned |
| `400` | Invalid path or the target is not a directory |
| `403` | The target cannot be read |
| `404` | The directory does not exist |
| `502` | Daemon error |
| `503` | No daemon is listening |

### `GET /api/files/content`

Returns an inline preview as raw bytes. The route reads until
`[files] max_preview_bytes` and then stops.

```bash
curl -s -D- "localhost:8080/api/files/content?path=/etc/hostname"
```

| Header | Meaning |
| --- | --- |
| `content-type` | `text/plain; charset=utf-8` for ordinary files; safe raster image types keep their MIME |
| `x-content-type-options` | Always `nosniff` |
| `content-disposition` | `inline; filename="..."` |
| `x-anth-size` | Full file size in bytes |
| `x-anth-truncated` | `true` when the preview stopped at the limit |

Inline previews are deliberately inert. HTML, SVG and every non-image type are
served as `text/plain`, and only raster images (`png`, `jpeg`, `gif`, `webp`,
`avif`, `bmp` and icons) keep an image MIME. This prevents a file in the browsed
tree from running script on the panel's origin.

### `GET /api/files/download`

Streams the whole file as an attachment. It uses the same chunked daemon
protocol, so files are not buffered in one socket frame.

```bash
curl -s -OJ "localhost:8080/api/files/download?path=/var/log/syslog"
```

The response carries `content-disposition: attachment` and `x-anth-size`.

### `POST /api/files/mkdir`

```json
{"path":"/var/tmp/example","recursive":false}
```

`recursive` defaults to `false`. Returns `{"path":"/var/tmp/example"}`.

### `POST /api/files/rename`

```json
{"from":"/var/tmp/a.txt","to":"/var/tmp/b.txt"}
```

Returns `{"from":"...","to":"..."}`. The route refuses to overwrite an existing
destination.

### `POST /api/files/delete`

```json
{"path":"/var/tmp/example","recursive":true}
```

`recursive` defaults to `false`. A non-empty directory without it is `409`.

### `POST /api/files/upload`

Uploads a raw request body. It is not multipart.

```bash
curl -s -X POST \
  "localhost:8080/api/files/upload?path=/var/tmp/a.bin&overwrite=false" \
  -H 'content-type: application/octet-stream' \
  --data-binary @a.bin
```

```json
{"path":"/var/tmp/a.bin","size":1234}
```

The API forwards each request-body frame to the daemon as it arrives, splitting
frames into 256 KiB chunks and enforcing `[files] max_upload_bytes` on the way.
The daemon writes a temporary file in the destination directory and renames it
only after the last chunk, so an interrupted upload does not replace the
destination.

| Status | When |
| --- | --- |
| `200` | Entry created or replaced |
| `400` | Invalid path or body |
| `403` | File writes are disabled, or the daemon cannot write |
| `404` | Parent directory does not exist |
| `409` | Destination exists and `overwrite=false` |
| `413` | Body or upload exceeds `max_upload_bytes` |
| `502` | Daemon error |

## Accounts

Read path is always available. Every mutation below requires
`[accounts] control = true` and answers `403` otherwise; the password route
requires `[accounts] manage_passwords = true` instead. See
[accounts.md](accounts.md).

### `GET /api/accounts`

```json
{
  "users": [
    {
      "name": "alice",
      "uid": 1001,
      "gid": 1001,
      "group": "alice",
      "gecos": "Alice Example",
      "home": "/home/alice",
      "shell": "/bin/sh",
      "locked": true,
      "system": false,
      "source": "local",
      "groups": ["alice", "developers"]
    }
  ],
  "groups": [
    { "name": "developers", "gid": 2000, "members": ["alice"], "system": false,
      "source": "local" }
  ],
  "control_enabled": true,
  "password_enabled": false,
  "timestamp_ms": 1700000000000,
  "total_users": 1,
  "total_groups": 1,
  "truncated": false,
  "warning": null
}
```

`source` is `local` or `nss`. `locked` is `null` when `/etc/shadow` could not be
read; it is never reported as `false` by default.

### `POST /api/accounts/users`

```json
{ "name": "alice", "uid": 1500, "gid": 1500, "gecos": "Alice",
  "home": "/home/alice", "shell": "/bin/sh", "system": false }
```

Only `name` is required. Every field is validated by the daemon's guards. A
new account is locked.

### `POST /api/accounts/users/{name}`

```json
{ "gecos": "Alice B", "shell": "/bin/bash", "home": "/home/alice",
  "primary_gid": 1500, "locked": true }
```

Omitted fields are unchanged. An empty object is `400 invalid`. A
`primary_gid` must name an existing group; a gid no group carries is
`404 not_found`. A gid below the host's `GID_MIN` is refused with
`403 protected` unless `accounts.allow_system` is enabled.

### `POST /api/accounts/users/{name}/delete`

```json
{ "remove_home": false }
```

The body may be empty. `remove_home` is only honoured when the home resolves
inside `accounts.home_root`.

### `POST /api/accounts/users/{name}/password`

```json
{ "password": "correct-horse-battery" }
```

Answers `403` unless `manage_passwords` is on. The password is not logged and
does not appear in the response.

### `POST /api/accounts/groups`

```json
{ "name": "developers", "gid": 2000, "system": false }
```

### `POST /api/accounts/groups/{name}`

```json
{ "gid": 2100, "members": ["alice", "bob"] }
```

`members` replaces the list. Both fields may be set together, which becomes a
`groupmod` followed by a `gpasswd`.

### `POST /api/accounts/groups/{name}/delete`

No body.

### Mutation responses

```json
{ "message": "created user alice" }
```

Failures use the shared `{"error": "..."}` shape. `invalid` is `400`,
`disabled` / `protected` / `permission_denied` are `403`, `not_found` is `404`,
`conflict` is `409`, `failed` is `500` and `unsupported` is `501`.

## `POST /api/echo`

Returns the message unchanged. Useful as a round-trip check of the whole chain
(HTTP → socket → daemon).

```bash
curl -s -X POST localhost:8080/api/echo \
  -H 'content-type: application/json' \
  -d '{"message":"hola"}'
```

```json
{"message":"hola"}
```

| Status | When |
| --- | --- |
| `200` | Message returned |
| `400` | Body is not JSON, or `message` is missing |
| `502` | The daemon answered with an error |
| `503` | No daemon is listening |

Note that `{"type":"Echo","data":{"message":"hola"}}` is **rejected**: the route
already names the command, so the envelope is not part of the HTTP contract.

## `GET /api/terminal`

Interactive terminal over a WebSocket. The handshake upgrades to a WebSocket and
the panel opens one PTY session per connection through the daemon. Closing the
WebSocket closes the session and kills its process group.

> Before this route is exposed beyond a trusted network, it needs authentication.
> The terminal token in the config is reserved for that follow-up; it is **not
> enforced** by this route yet. Keep `anth-panel api`/`run` on loopback or behind
> a reverse proxy that authenticates users.

### Query parameters

| Parameter | Default | Description |
| --- | --- | --- |
| `cols` | `80` | Initial terminal width in character cells |
| `rows` | `24` | Initial terminal height in character cells |

Provide both or neither. Sizes outside `1x1..1000x1000` are rejected with `400`
before the upgrade; only one of the pair is also a `400`.

### Handshake

```
GET /api/terminal?cols=120&rows=40 HTTP/1.1
Upgrade: websocket
Connection: Upgrade
...
```

A successful upgrade is `101 Switching Protocols`. Every message is a JSON text
frame; binary frames are rejected. Terminal bytes are base64 encoded.

### Browser → panel

```json
{"type":"input","data":"bHMK"}
{"type":"resize","cols":120,"rows":40}
{"type":"ping"}
```

| Frame | Meaning |
| --- | --- |
| `input` | Bytes typed into the terminal. `data` is base64. |
| `resize` | New window size. The daemon signals `SIGWINCH`. |
| `ping` | Application-level keep-alive. Answered with `pong`. |

A successful input or resize is silent. A 64 KiB decoded input limit applies;
larger frames get an `error` frame and the connection closes.

### Panel → browser

```json
{"type":"ready","shell":"/bin/sh","cols":120,"rows":40,"line_editing":false}
{"type":"output","data":"aGVsbG8K"}
{"type":"exit","code":0}
{"type":"error","detail":"too many terminal sessions"}
{"type":"pong"}
```

| Frame | Meaning |
| --- | --- |
| `ready` | Always the first frame. Shell program, actual size, and whether the shell has native line editing/Tab completion. |
| `output` | Terminal bytes, base64 encoded. |
| `exit` | The program ended. Sent immediately before the socket closes. |
| `error` | The session failed. Sent before the socket closes. |
| `pong` | Reply to an application-level `ping`. |

`exit` and `error` are terminal: no more frames follow. A daemon that is down
still lets the WebSocket upgrade; the browser receives an `error` frame instead
of an unobservable close code.

The same socket protocol and `TerminalSize` limits are documented in
[socket-protocol.md](socket-protocol.md#terminal).

## Adding a route

`POST /api/echo` rewrites to a body like this because each command maps to a
route. To add one:

1. Add a `Deserialize` struct with the command's arguments as top-level fields.
2. Add the route in `router()`.
3. Send the matching `anth_proto::Request` and serialize the result.

For a streaming variant, reuse `poll::stream` for the cadence and
`SseStream::new("<event-name>")` for the framing rather than writing either
again.
