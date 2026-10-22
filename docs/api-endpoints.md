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
| `anth-panel process spawn /bin/ls -la` | `POST /api/processes/spawn` with `{"program":"/bin/ls","args":["-la"]}` |
| `anth-panel network list` | `GET /api/network` |
| `anth-panel network list --watch 2s` | `GET /api/network/watch?interval=2s` |
| `anth-panel network down eth0` | `POST /api/network/eth0/state` with `{"up":false}` |
| `anth-panel network create br0 --kind bridge` | `POST /api/network` with `{"name":"br0","kind":"bridge"}` |
| `anth-panel network edit eth0 --mtu 9000` | `PATCH /api/network/eth0` with `{"mtu":9000}` |
| `anth-panel network rename d0 d1` | `POST /api/network/d0/rename` with `{"new_name":"d1"}` |
| `anth-panel network delete d0` | `POST /api/network/d0/delete` |
| `anth-panel firewall status` | `GET /api/firewall` |
| `anth-panel firewall allow 22/tcp` | `POST /api/firewall/rules` with `{"action":"allow","port":22}` |
| `anth-panel firewall delete 3` | `POST /api/firewall/rules/3/delete` |
| `anth-panel firewall flush` | `POST /api/firewall/flush` |
| `anth-panel volume list` | `GET /api/volumes` |
| `anth-panel volume mount /dev/sdb1 /mnt/data` | `POST /api/volumes/mount` with `{"device":"/dev/sdb1","target":"/mnt/data"}` |
| `anth-panel volume unmount /mnt/data` | `POST /api/volumes/unmount` with `{"target":"/mnt/data"}` |
| `anth-panel volume partitions /dev/sdb` | `GET /api/volumes/partitions?disk=/dev/sdb` |
| `anth-panel volume partition create /dev/sdb --size 1G` | `POST /api/volumes/partitions` |
| `anth-panel volume partition delete /dev/sdb 1` | `POST /api/volumes/partitions/edit` with `{"action":"delete",...}` |
| `anth-panel volume partition format /dev/sdb1 --type ext4` | `POST /api/volumes/partitions/edit` with `{"action":"format",...}` |
| `anth-panel file list /tmp` | `GET /api/files?path=/tmp` |
| `anth-panel file read /tmp/a` | `GET /api/files/content?path=/tmp/a` |
| `anth-panel file mkdir /tmp/a` | `POST /api/files/mkdir` with `{"path":"/tmp/a"}` |
| `anth-panel file rename /a /b` | `POST /api/files/rename` with `{"from":"/a","to":"/b"}` |
| `anth-panel file delete /a` | `POST /api/files/delete` with `{"path":"/a"}` |
| `anth-panel file trash /a` | `POST /api/files/trash` with `{"path":"/a"}` |
| `anth-panel file trash-bin list` | `GET /api/files/trash/list` |
| `anth-panel file trash-bin restore a.txt` | `POST /api/files/trash/restore` with `{"id":"a.txt"}` |
| `anth-panel file trash-bin empty` | `POST /api/files/trash/empty` |
| `anth-panel file archive /a` | `POST /api/files/archive` with `{"path":"/a"}` |
| `anth-panel file unarchive --list /a.zip` | `GET /api/files/archive/list?path=/a.zip` |
| `anth-panel file unarchive /a.zip` | `POST /api/files/archive/extract` with `{"path":"/a.zip"}` |
| `anth-panel file write /a.txt b.txt` | `POST /api/files/write?path=/a.txt` with a raw body |
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
| `POST` | `/api/processes/spawn` | Start a program from `[process] spawn_allow` |
| `GET` | `/api/network` | One network-interface snapshot |
| `GET` | `/api/network/watch` | Network snapshots as SSE |
| `POST` | `/api/network/{name}/state` | Bring an interface up or down |
| `POST` | `/api/network` | Create one virtual interface |
| `PATCH` | `/api/network/{name}` | Change an MTU or the administrative state |
| `POST` | `/api/network/{name}/rename` | Rename an interface that is down |
| `POST` | `/api/network/{name}/delete` | Delete an interface |
| `GET` | `/api/firewall` | One managed firewall snapshot |
| `POST` | `/api/firewall/rules` | Add a deny or remove a managed deny |
| `POST` | `/api/firewall/rules/{handle}/delete` | Delete a rule by handle |
| `POST` | `/api/firewall/flush` | Flush the managed chain |
| `GET` | `/api/volumes` | One storage-inventory snapshot |
| `POST` | `/api/volumes/mount` | Mount a device on a directory |
| `POST` | `/api/volumes/unmount` | Unmount a mounted directory |
| `GET` | `/api/volumes/partitions` | One disk's partition table |
| `POST` | `/api/volumes/partitions` | Create a partition |
| `POST` | `/api/volumes/partitions/edit` | Resize, delete or format a partition |
| `GET` | `/api/files` | One directory listing |
| `GET` | `/api/files/content` | Inline file preview |
| `GET` | `/api/files/download` | File download |
| `POST` | `/api/files/mkdir` | Create a directory |
| `POST` | `/api/files/rename` | Rename an entry |
| `POST` | `/api/files/delete` | Delete an entry permanently |
| `POST` | `/api/files/trash` | Move an entry to the freedesktop trash |
| `GET` | `/api/files/trash/list` | List every trash directory |
| `POST` | `/api/files/trash/restore` | Restore one trashed entry |
| `POST` | `/api/files/trash/empty` | Delete every trashed entry |
| `POST` | `/api/files/archive` | Compress one entry |
| `GET` | `/api/files/archive/list` | List an archive's members |
| `POST` | `/api/files/archive/extract` | Extract an archive |
| `POST` | `/api/files/write` | Save text over an existing file (raw body) |
| `POST` | `/api/files/upload` | Upload a raw body |
| `GET` | `/api/auth/session` | Whether the session is open |
| `POST` | `/api/auth/login` | Exchange a password for the token pair |
| `POST` | `/api/auth/refresh` | Rotate the access and refresh tokens |
| `POST` | `/api/auth/logout` | Revoke the session and clear the cookies |
| `POST` | `/api/auth/password` | Change the panel password |
| `GET` | `/api/config` | The editable configuration view |
| `PATCH` | `/api/config` | Change configuration fields |

Everything under `/api` except `/api/auth/login`, `/api/auth/refresh` and
`/api/auth/session` requires a session when `[auth] enabled` is true; see
[Authentication](#authentication). Anything under `/api` that is not a route
listed above answers `404` with the usual `{"error": ...}` body. Every other path falls through to the embedded web
UI, which serves static files and returns `index.html` for unknown paths so the
SPA can route.

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
  "spawn_enabled": false,
  "spawn_allow": [],
  "total": 1,
  "truncated": false
}
```

| Field | Meaning |
| --- | --- |
| `clock_ticks` | `sysconf(_SC_CLK_TCK)`, for converting tick deltas to CPU percent |
| `timestamp_ms` | Wall-clock time of the snapshot |
| `control_enabled` | Whether `[process] control` allows signals |
| `spawn_enabled` | Whether `[process] spawn` allows starting programs |
| `spawn_allow` | Absolute programs the daemon will start; empty denies every spawn |
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

## `POST /api/processes/spawn`

Starts a program from the daemon allowlist.

```bash
curl -s -X POST localhost:8080/api/processes/spawn \
  -H 'content-type: application/json' \
  -d '{"program":"/bin/ls","args":["-la","/tmp"],"cwd":"/tmp"}'
```

```json
{"pid":4321,"program":"/bin/ls","args":["-la","/tmp"]}
```

`args` and `cwd` are optional. `program` must be an absolute path that
canonicalises to an entry in `[process] spawn_allow`, and `[process] spawn` must
be `true`; an empty allowlist denies every request. The program is executed
directly, never through a shell, with stdin closed and output discarded. The
daemon does not supervise the child.

| Status | When |
| --- | --- |
| `200` | Process started; the body carries its pid |
| `400` | Invalid body, or the daemon refused for another reason |
| `403` | Spawning is disabled, the program is not in the allowlist, is not absolute, is not executable, or cannot be read |
| `404` | The program or the working directory does not exist |
| `502` | The daemon answered with an unexpected error |
| `503` | No daemon is listening on the socket |

See [processes.md](processes.md) for the allowlist rules. `PATCH /api/config`
writes the new allowlist to the file, but a running daemon keeps the policy it
read at startup, so a change takes effect after a restart.

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

## `POST /api/network`

Creates one virtual interface. Kinds: `veth`, `bridge`, `bond`, `vlan`, `dummy`,
`tun_tap`. Everything else is `400`.

```bash
curl -s -X POST localhost:8080/api/network \
  -H 'content-type: application/json' \
  -d '{"name":"br0","kind":"bridge","mtu":1500,"up":true}'
```

```json
{"message":"created bridge interface `br0`"}
```

A `veth` takes `peer`; a `vlan` takes `link` and `vlan_id` (1-4094). The route
returns `403` when `[network] manage = false`, `400` for an illegal name, an
existing name, an out-of-range MTU or a kind that cannot be created, and `404`
for a missing vlan parent.

## `PATCH /api/network/{name}`

Changes the MTU and/or the administrative state. An absent field is left alone,
so `{"mtu":9000}` never takes the link down.

```bash
curl -s -X PATCH localhost:8080/api/network/eth0 \
  -H 'content-type: application/json' \
  -d '{"mtu":9000}'
```

```json
{"message":"updated `eth0`: mtu 9000"}
```

An empty body is `400` ("nothing to change"). Bringing a loopback down is `403`.

## `POST /api/network/{name}/rename`

Renames an interface. It must be down; the daemon answers `403` otherwise.

```bash
curl -s -X POST localhost:8080/api/network/d0/rename \
  -H 'content-type: application/json' \
  -d '{"new_name":"d1"}'
```

```json
{"message":"renamed `d0` to `d1`"}
```

## `POST /api/network/{name}/delete`

Deletes an interface, including a physical device.

```bash
curl -s -X POST localhost:8080/api/network/d0/delete
```

```json
{"message":"deleted interface `d0`"}
```

The route forwards the connection's peer address to the daemon, which refuses
(`403`) to delete the interface that request arrived through, the last interface
carrying a default route, or a loopback. The peer guard is skipped when the
address is loopback or matches no interface, where it says nothing about the
path. Deleting a physical device is allowed because removing a card on purpose
is legitimate; the CLI and the web UI warn about it separately.

| Status | When |
| --- | --- |
| `200` | The interface was created, edited, renamed or deleted |
| `400` | Invalid name, body or parameters |
| `403` | `[network] manage` is off, a guard refused the change, or permission was denied |
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

### `GET /api/volumes/partitions`

```bash
curl -s 'localhost:8080/api/volumes/partitions?disk=/dev/sdb'
```

```json
{
  "disk": "/dev/sdb",
  "partitions": [
    {"number": 1, "path": "/dev/sdb1", "size_bytes": 1073741824,
     "fstype": "ext4", "mountpoints": ["/mnt/data"]}
  ],
  "control_enabled": true,
  "format_enabled": false,
  "format_types": ["ext4", "xfs", "vfat", "exfat", "btrfs", "swap"],
  "allowed_disks": ["/dev/sdb"],
  "warning": null
}
```

`disk` must name a whole disk. Reading a table is allowed whenever
`[volumes] control` is on; it is not narrowed by `disk_allow`, because it
changes nothing. `control_enabled` and `format_enabled` let a client hide an
action instead of offering one certain to fail.

| Status | When |
| --- | --- |
| `200` | Table read |
| `404` | `disk` is not a known whole disk |

### `POST /api/volumes/partitions`

```bash
curl -s -X POST localhost:8080/api/volumes/partitions \
  -H 'content-type: application/json' \
  -d '{"disk":"/dev/sdb","kind":"linux","start_bytes":0,"size_bytes":1073741824}'
```

```json
{"message":"created a linux partition of 1073741824 bytes on /dev/sdb"}
```

`kind` is `linux`, `swap` or `efi`. `size_bytes` must be a non-zero whole
number of 512-byte sectors; `start_bytes` may be `0` to let `sfdisk` place the
partition.

| Status | When |
| --- | --- |
| `200` | Created |
| `400` | Bad path, unknown kind, or a size that is not whole sectors |
| `403` | `[volumes] partition` is off, the disk is not in `disk_allow`, or the disk carries a protected or any mounted partition |
| `404` | The disk is not in the inventory |
| `409` | The disk has a mounted partition |
| `500` | `sfdisk` failed |

### `POST /api/volumes/partitions/edit`

One route for the three remaining actions, tagged by `action`:

```json
{"action":"resize","disk":"/dev/sdb","number":1,"size_bytes":2147483648}
{"action":"delete","disk":"/dev/sdb","number":1}
{"action":"format","partition":"/dev/sdb1","fs_type":"ext4","label":"data"}
```

`format` requires `[volumes] format` in addition to `[volumes] partition`; the
guard runs before the daemon is asked, so the error names the switch. A
`format` target must be a partition of an allowlisted disk and must be
unmounted.

| Status | When |
| --- | --- |
| `200` | Applied |
| `400` | Unknown `action`, bad path, size not whole sectors, unsupported `fs_type`, or a label with an unsafe character |
| `403` | `[volumes] partition` or `[volumes] format` is off, or the disk is not in `disk_allow` |
| `404` | The disk or partition is not in the inventory |
| `409` | The partition or its disk is mounted |
| `500` | The tool failed |

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

> With `[auth] enabled` this upgrade needs a session like every other `/api`
> route, so a browser that has not signed in cannot open a PTY. The per-session
> `terminal.token` is still **not enforced**; it is reserved for a capability
> that outlives the panel session. The daemon socket itself stays
> unauthenticated, so keep `anth-panel api`/`run` on loopback or behind a proxy
> that authenticates users when the host is not trusted.

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

## `GET /api/plugins`

Read-only view of the installed plugin set. It does not travel over the socket:
the daemon has no opinion about which plugins the panel shows, so the route
reads the bundle directory and the grants directly.

```json
{
  "enabled": true,
  "install_from_web": false,
  "plugins": [
    {
      "id": "org.anth.wireguard",
      "version": "0.1.0",
      "name": "WireGuard",
      "enabled": true,
      "capabilities": ["wg.status"],
      "api": [{"method": "GET", "path": "/status"}],
      "ui": [{"slot": "dashboard.widget", "tag": "anth-wg-card", "module": "web/card.js"}]
    }
  ],
  "problems": []
}
```

`problems` holds one line per plugin that could not load — a directory with no
grant, or a module whose hash no longer matches. The view reports them instead
of hiding them, because a plugin that silently fails to load looks like a bug in
the panel. `enabled` reflects `[plugins] enabled`, so a UI can distinguish "no
plugins installed" from "plugins turned off".

Installing from the browser is gated by `[plugins] install_from_web`, which is
off by default: installing means running code.

## `/api/plugins/{id}/{*path}`

Every anchor a plugin answers, under one wildcard route. `GET`, `POST`, `PUT`,
`PATCH` and `DELETE` are all routed to the same handler, and the plugin answers
from the envelope it receives.

The handler never reads the manifest. That is the design, not an omission: a
manifest is a file inside the bundle, so a rule that only consulted it would be
the plugin deciding which of its own routes are reachable. What authorizes the
call is the grant — the same `anth_exec` guard the CLI path uses — so a request
for a path the manifest did not declare still reaches the guest, and still
cannot run anything the grant does not freeze.

The envelope is the one documented in
[design/plugins.md](design/plugins.md#api-anchors):

```json
{"kind": "http", "method": "GET", "path": "/status", "query": {"peer": "wg1"}, "body": null}
```

`path` is always the anchor as the manifest writes it: `.../status` and
`...//status` and `.../status/` all arrive as `/status`, and `.../wg/peers` as
`/wg/peers`. axum hands a `{*path}` capture over with the leading slash stripped,
so without this a guest dispatching on its declared anchors would match only the
doubled-slash spelling — the one no client sends.

The guest answers with an envelope carrying its own status and body:

```json
{"status": 200, "body": "ok"}
```

A body that is not an envelope is passed through as `200 text/plain`, because a
plugin returning a plain string is valid and the host will not invent a status
for it.

Status codes:

| Status | When |
| --- | --- |
| `200`, or the guest's own status | the guest answered |
| `400` | the id is not a valid plugin id |
| `401` | no valid access token, when `[auth] enabled` |
| `403` | the grant exists but `enabled = false`; never a bundle on disk |
| `404` | `[plugins] enabled = false`, or no grant for that id |
| `405` | a method other than the five above |
| `413` | the request body is larger than 1 MiB |
| `422` | the bundle no longer matches its grant, no longer parses, or its module is invalid |
| `502` | the guest failed; it is the plugin that is broken, not the caller |
| `504` | the guest ran out of fuel or passed the 30 s deadline |

A declared path is resolved through `anchor_route`, which normalizes it and
refuses `..`, a leading `//` and an invalid id. `Manifest::parse` applies that
rule, and `install` and `load` both parse, so a manifest rewritten on disk after
install is refused before its module is read. The refusal is a check on the
manifest's honesty; it is not the routing rule.

## `GET /api/plugins/{id}/web/{*path}`

A plugin's interface assets, from its bundle's `web/` directory. More specific
than the anchor wildcard, so an asset request never reaches the guest: the host
reads the file itself, because running wasm to answer for a static file it cannot
read would buy nothing.

The response carries a fixed `content-type` from a host-owned extension table
(`text/javascript`, `text/css`, `application/json`, `image/svg+xml`, the raster
types, the font types and `application/wasm`; anything else is
`application/octet-stream`), `x-content-type-options: nosniff`, and
`cache-control: no-store`. The last one is deliberate: a cached module would
outlive an uninstall, and a reinstall reuses the same id.

A disabled plugin is `403` and a bundle without a grant is `404`, because the
grant is what the admin approved and the directory is only content.

Status codes:

| Status | When |
| --- | --- |
| `200` | the asset was read |
| `400` | the id is not a valid plugin id, or the path contains a `..` segment (encoded or not) |
| `401` | no valid access token, when `[auth] enabled` |
| `403` | the grant exists but `enabled = false` |
| `404` | `[plugins] enabled = false`, no grant, no `web/` directory, no such file, or the path resolves outside `web/` |
| `413` | the asset is larger than 4 MiB |

The containment rule has three parts. The capture is rebuilt component by
component, so `..` is refused after axum has already decoded `%2e%2e` and `a..b`
stays a legal name. The resolved path is `canonicalize`d *before* the
`starts_with` check, so a symlink planted after install is resolved rather than
followed. And a directory is never a listing.

## Adding a route

`POST /api/echo` rewrites to a body like this because each command maps to a
route. To add one:

1. Add a `Deserialize` struct with the command's arguments as top-level fields.
2. Add the route in `router()`.
3. Send the matching `anth_proto::Request` and serialize the result.

For a streaming variant, reuse `poll::stream` for the cadence and
`SseStream::new("<event-name>")` for the framing rather than writing either
again.

## Authentication

`[auth] enabled = true` puts a middleware in front of every `/api` route. It
accepts either credential:

- the `anth_access` cookie, which the browser sends by itself;
- an `Authorization: Bearer <token>` header, for a script or a client that does
  not keep cookies.

`/healthz` stays open so a container probe keeps working, and the three login
routes stay open because requiring a session to log in would deadlock.

A rejection is `401` with `{"error": "authentication required"}`. A token that
is well formed but expired gives `{"error": "session expired"}` so the UI knows
to try a refresh before showing the login screen.

Both cookies are `HttpOnly` and `SameSite=Strict`; add `Secure` with
`[auth] cookie_secure = true` once the panel is behind HTTPS. `SameSite=Strict`
is what makes the panel safe without a CSRF token: a cross-site form post never
carries the cookie.

### `GET /api/auth/session`

Reports the login state, never a secret.

```json
{
  "enabled": true,
  "authenticated": true,
  "username": "admin",
  "access_expires_in": 870,
  "refresh_expires_in": 43100
}
```

`access_expires_in` and `refresh_expires_in` are **seconds remaining**, both `0`
when that token is absent. A tab opened halfway through an access token reports
the time left, not the configured lifetime, so its silent refresh is scheduled
correctly. The shell uses them to arm a silent refresh slightly
before the access token expires.

`authenticated` is `true` only when the presented cookie **verifies and names a
live session**. A stale or forged cookie reports `false`, so the shell shows the
login instead of rendering panels that every `/api` call would reject.

### `POST /api/auth/login`

```json
{ "username": "admin", "password": "..." }
```

Sets both cookies and answers:

```json
{ "username": "admin", "role": "admin", "access_expires_in": 900 }
```

The UI then reads `GET /api/auth/session` for the rest of the state. A wrong user
and a wrong password answer the same `401 {"error": "invalid credentials"}` so
the route cannot be used to enumerate the account name.

### `POST /api/auth/refresh`

Takes the refresh cookie, nothing in the body. Rotates the pair: the presented
refresh token is spent and a new one is issued. Rotation does not extend the
login: the new refresh token keeps the session's original expiry.

The check, the spend and the re-issue happen under one lock, so two callers
holding the same token get exactly one `200` between them and the loser sees the
session cancelled. A refresh token is not an API credential: it is rejected by
the same check that keeps the two halves apart, so a leaked one cannot be used
as a Bearer token for its whole life.

Presenting a token that is not the current one for a live session answers `401`
and cancels that session. That covers both a rotated token being replayed and an
access token offered in place of a refresh token, and in both cases one of the
two holders is an intruder. The honest client is signed out too, which is the
point: the panel cannot tell which copy is genuine.

### `POST /api/auth/logout`

Revokes the session, if the refresh cookie names one, and clears both cookies
with `Max-Age=0`. It is safe to call without a session.

### `POST /api/auth/password`

```json
{ "current_password": "...", "new_password": "..." }
```

Requires a session. The new password must be at least 8 characters.

The change rotates the signing key and drops every session, then clears both
cookies. Two consequences follow, and both are deliberate:

- an access token handed out before the change stops working **immediately**
  rather than when it expires, because the service that signed it is gone;
- the caller is signed out like everyone else. A password change is what an
  operator does when a credential may be compromised, so leaving one working
  token behind would defeat it.

### `GET /api/config` and `PATCH /api/config`

The view is the TOML file without secrets: `[auth] password_hash` and
`[terminal] token` are reported as `password_set` and `token_set` booleans.

A patch replaces only the fields it names; an absent key keeps its value. The
result is serialized and re-parsed before it is stored, so a patch that would
produce an unloadable file is rejected with `400`. It is then applied in memory
and written to the file the process was started with.

`{"auth": {"enabled": false}}` is refused. Turning the password requirement off
is a host decision, made with `anth-panel auth enable --disable`, so a stolen
session cannot remove the lock from the panel itself.

```json
{ "files": { "write": true }, "volumes": { "mount_root": "/srv" } }
```
