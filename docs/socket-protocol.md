# Socket protocol

`anth-panel` (and any custom tool) talks to the daemon over a Unix stream
socket. This document is the contract for that transport, so an alternative
client or daemon can be written against it.

The reference implementation is the `anth-proto` crate.

## Transport

| Property | Value |
| --- | --- |
| Socket type | Unix stream socket (`SOCK_STREAM`) |
| Default path | `/var/tmp/anth.sock`, overridable in the config file |
| Framing | Newline-delimited JSON: one JSON object per line, `\n` terminated |
| Encoding | UTF-8 |
| Direction | Full duplex on one connection |
| Max line | 1 MiB (`MAX_LINE_LEN`); a longer line is a protocol error |

A line is a complete message. A message must not contain a raw newline; JSON
encoding never produces one, so `\n` is always a frame boundary.

Binary data — terminal input/output and file chunks — travels as a **base64
string**, never as raw bytes. The encoding is stated per member below. A raw byte
in a frame would either break UTF-8 or, for `0x0a`, be read as a frame boundary.

## Request and response shape

Both messages are JSON objects with two members:

```json
{ "type": "<Variant>", "data": <payload> }

```

`type` names the operation and `data` carries its arguments. The `data` member
is omitted entirely for variants that take no arguments (`HostInfo`,
`ProcessList`, `NetworkList`).

The pair is request/response: the client writes one request and reads one
response on the same connection. A connection may carry several request/response
pairs in sequence; the daemon serves them in order.

The terminal is the one exception. `TerminalOpen` is answered once with
`TerminalOpened`, but `TerminalOutput` frames then arrive **unsolicited**, as the
program produces bytes. A client must therefore read continuously rather than
assuming one response per request, and must tolerate output interleaving with the
acknowledgements of its own requests.

> Note: this envelope is specific to the socket transport. The HTTP API omits it
> and names the command in the route instead — see [api-endpoints.md](api-endpoints.md).
> The browser terminal route has its own JSON frames, also documented there.

## Requests

### `Echo`

Returns the message unchanged. Useful as a liveness check.

```json
{"type":"Echo","data":{"message":"hola"}}

```

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `message` | string | yes | Text to return |

### `HostInfo`

Returns a snapshot of the host the daemon runs on. Takes no arguments, so the
`data` member is absent.

```json
{"type":"HostInfo"}

```

The snapshot never inspects individual processes.

### `ProcessList`

Returns one snapshot of the process table. Takes no arguments, so the `data`
member is absent.

```json
{"type":"ProcessList"}

```

### `ProcessSignal`

Delivers an allowlisted signal to one process.

```json
{"type":"ProcessSignal","data":{"pid":1234,"signal":"term"}}

```

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `pid` | integer | yes | Target process id; `1` and the daemon's own pid are refused |
| `signal` | string | yes | `term`, `kill`, `hup`, `int`, `quit`, `stop` or `cont` |

The daemon refuses signals unless `[process] control = true`, the target belongs
to the daemon user, and the pid is not protected. See
[processes.md](processes.md) for the policy.

Reply: `ProcessSignalled`.

### `NetworkList`

Returns one snapshot of every network interface. Takes no arguments, so the
`data` member is absent.

```json
{"type":"NetworkList"}

```

### `NetworkSetState`

Brings one interface up or down.

```json
{"type":"NetworkSetState","data":{"name":"eth0","up":false}}

```

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | string | yes | Interface name from the snapshot |
| `up` | boolean | yes | `true` to bring it up, `false` to bring it down |

The daemon refuses the change unless `[network] control = true`. Invalid names,
unknown interfaces and bringing `lo` down are rejected. See
[network.md](network.md) for the policy.

Reply: `NetworkStateChanged`, or `NetworkError`.

### Filesystem requests

Paths are virtual: `/` is the daemon's `[files] root`. `..` components are
invalid, and reads follow symlinks only after checking the canonical target
stays inside the root. Writes require `[files] write = true`.

### `FileList`

```json
{"type":"FileList","data":{"path":"/var/log"}}

```

Reply: `FileListing`.

### `FileRead`

Reads one stateless chunk. Advance `offset` until `eof` is `true`.

```json
{"type":"FileRead","data":{"path":"/etc/hostname","offset":0,"len":262144}}

```

`len` is clamped to 512 KiB. Reply: `FileChunk`.

### `FileMkdir`

```json
{"type":"FileMkdir","data":{"path":"/var/tmp/example","recursive":false}}

```

Reply: `FileCreated`.

### `FileRename`

```json
{"type":"FileRename","data":{"from":"/var/tmp/a","to":"/var/tmp/b"}}

```

Reply: `FileRenamed`. The daemon refuses an existing destination.

### `FileDelete`

```json
{"type":"FileDelete","data":{"path":"/var/tmp/example","recursive":true}}

```

Reply: `FileDeleted`. A non-empty directory requires `recursive`.

### `FileWriteStart`

Opens an atomic upload and replies with an id.

```json
{"type":"FileWriteStart","data":{"path":"/var/tmp/a.bin","overwrite":false}}

```

Reply: `FileWriteStarted`.

### `FileWriteChunk`

```json
{"type":"FileWriteChunk","data":{"id":1,"bytes":"AAEC/w=="}}

```

`bytes` is base64. One chunk is capped at 512 KiB; clients normally send 256 KiB.
Reply: `FileWriteAccepted` with the running byte count.

### `FileWriteFinish`

```json
{"type":"FileWriteFinish","data":{"id":1}}

```

Flushes, closes and renames the temporary file. Reply: `FileWritten`.

### `FileWriteAbort`

```json
{"type":"FileWriteAbort","data":{"id":1}}

```

Discards the upload and removes its temporary file. Reply: `FileWriteAborted`.
Dropping the connection has the same effect.

## Responses

### `Ok`

Success result for a request that produces a scalar value.

```json
{"type":"Ok","data":{"message":"hola"}}

```

| Field | Type | Description |
| --- | --- | --- |
| `message` | string | Resulting text |

### `Error`

The daemon rejected the request or failed to serve it. `anthd` sends this both
for a well-formed request it could not complete and for input it could not
parse.

```json
{"type":"Error","data":{"detail":"Error parsing request: expected value at line 1 column 1"}}

```

| Field | Type | Description |
| --- | --- | --- |
| `detail` | string | Human-readable reason |

### `ProcessSnapshot`

Reply to `ProcessList`.

```json
{
  "type": "ProcessSnapshot",
  "data": {
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
}
```

`cpu_ticks` is `utime + stime`. `clock_ticks` (`sysconf(_SC_CLK_TCK)`) and
`timestamp_ms` let a client compute CPU percent between two snapshots. Use
`pid` + `start_ticks` as the process identity because pids can be reused.

The daemon returns at most 2000 processes, sorted by accumulated CPU. To keep
every snapshot within the 1 MiB line cap, command lines are shortened (first to
256 characters, then further if needed) and, as a last resort, the least busy
rows are dropped. Command shortening sets `truncated` without a warning;
dropping rows also adds a human-readable `warning`. `total` reports the
untruncated count. Clients must not treat the list as complete without checking
those fields.

### `ProcessSignalled`

Successful reply to `ProcessSignal`.

```json
{"type":"ProcessSignalled","data":{"pid":1234,"signal":"term"}}

```

A refused signal arrives as an `Error` response instead.

### `NetworkSnapshot`

Reply to `NetworkList`.

```json
{
  "type": "NetworkSnapshot",
  "data": {
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
}
```

`loopback` marks an `IFF_LOOPBACK` interface; the daemon refuses to bring one
down. The daemon returns at most 2000 interfaces and shrinks the list further to
fit the 1 MiB line cap; `total` reports the untruncated count and a shortened
list adds a human-readable `warning`.
Traffic counters are cumulative. `timestamp_ms` lets a client compute
RX/TX rates between consecutive snapshots; the daemon does not keep a sampler.

### `NetworkStateChanged`

Successful reply to `NetworkSetState`.

```json
{"type":"NetworkStateChanged","data":{"name":"eth0","up":false}}

```

A refused change arrives as `NetworkError` with `kind` (`disabled`, `protected`,
`not_found`, `permission_denied`, `invalid` or `failed`) and `detail`.

### File responses

`FileListing`, `FileChunk`, `FileWritten`, `FileCreated`, `FileDeleted`,
`FileRenamed`, `FileWriteStarted`, `FileWriteAccepted` and `FileWriteAborted`
mirror the requests above. `FileListing` is:

```json
{
  "type": "FileListing",
  "data": {
    "path": "/",
    "parent": null,
    "entries": [
      {
        "name": "a.txt",
        "path": "/a.txt",
        "kind": "file",
        "size": 3,
        "modified_ms": 1700000000000,
        "hidden": false
      }
    ],
    "total": 1,
    "truncated": false,
    "write_enabled": false
  }
}
```

`kind` is `file`, `directory`, `symlink` or `other`; symlinks also carry
`target`. `FileChunk` is:

```json
{
  "type": "FileChunk",
  "data": {
    "path": "/etc/hostname",
    "offset": 0,
    "size": 12,
    "eof": true,
    "bytes": "Ym94Cg=="
  }
}
```

Filesystem failures use a typed response so HTTP clients can map status codes:

```json
{
  "type": "FileError",
  "data": {
    "kind": "not_found",
    "detail": "`/missing`: No such file or directory (os error 2)"
  }
}
```

`kind` is one of `not_found`, `permission_denied`, `not_a_directory`,
`is_a_directory`, `invalid_path`, `already_exists`, `directory_not_empty`,
`too_large`, `disabled` or `io`.

### `HostInfo` (reply)

Reply to a `HostInfo` request.

```json
{
  "type": "HostInfo",
  "data": {
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
}

```

| Field | Type | Unit | Notes |
| --- | --- | --- | --- |
| `hostname` | string | — | From the kernel |
| `os.name` | string | — | Distribution name |
| `os.version` | string | — | May be empty |
| `os.kernel` | string | — | Kernel release |
| `os.arch` | string | — | Machine architecture |
| `uptime_seconds` | integer | seconds | Since boot |
| `cpu_model` | string | — | First `model name` reported |
| `logical_cores` | integer | count | `cpuinfo` entries |
| `load.one` | float | — | 1-minute load average |
| `load.five` | float | — | 5-minute load average |
| `load.fifteen` | float | — | 15-minute load average |
| `memory.total_kib` | integer | KiB | Total RAM |
| `memory.available_kib` | integer | KiB | Estimate of available RAM |
| `memory.swap_total_kib` | integer | KiB | `0` when swap is off |
| `memory.swap_free_kib` | integer | KiB | `0` when swap is off |

Values the daemon cannot read degrade to `unknown` for strings and `0` for
numbers; the response is still valid. Treat `0` swap as "not configured" rather
than "none in use".

## Sequence

```text
client                              daemon
  |  {"type":"HostInfo"}\n            |
  |--------------------------------->|
  |                                   |
  |  {"type":"HostInfo","data":{...}}\n|
  |<---------------------------------|
  |  {"type":"HostInfo"}\n            |    same connection may continue
  |--------------------------------->|

```

Because the protocol is one response per request, a client can request the same
operation repeatedly on one connection. That is how streaming works: the CLI
`--watch` flag and the SSE endpoint both re-send a request per tick and keep the
socket open.

## Errors a client must handle

| Situation | What arrives |
| --- | --- |
| Unknown `type` | `Error` with a parse message, then the connection closes |
| `data` missing for a variant that needs it | `Error` with a parse message, then the connection closes |
| Malformed JSON line | `Error` with a parse message, then the connection closes |
| Filesystem operation failed | `FileError` with `kind` and `detail`; the connection stays open |
| Network change failed | `NetworkError` with `kind` and `detail`; the connection stays open |
| Daemon goes away | The connection closes with no message; a write fails with `Broken pipe` |

Dropping the connection is the daemon's only way to end a conversation, so a
client should treat EOF as an error rather than an empty response.

## Terminal

A terminal session is a program running on a pseudo-terminal. Sessions are named
by an `id` that the daemon assigns, and are owned by the connection that opened
them: when that connection ends, its sessions are killed. A session opened on one
connection cannot be used from another.

Byte-carrying members (`bytes`) are base64. All of them are opaque: the daemon
does not interpret them, so ANSI sequences and control characters pass through
untouched.

### `TerminalOpen`

```json
{"type":"TerminalOpen","data":{"size":{"cols":80,"rows":24}}}

```

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `size.cols` | number | no | Columns, 1..1000. Omitted means 80. |
| `size.rows` | number | no | Rows, 1..1000. Omitted means 24. |

Reply: `TerminalOpened`.

```json
{"type":"TerminalOpened","data":{"id":1,"size":{"cols":80,"rows":24},"program":"/bin/sh","line_editing":false}}

```

`size` is the size the session actually started with, and `program` the program
that was spawned. `line_editing` reports whether the shell is known to provide
interactive line editing and Tab completion; old daemons omit it and clients
must default it to `false`. A size outside 1..1000 is refused rather than
clamped.

### `TerminalInput`

```json
{"type":"TerminalInput","data":{"id":1,"bytes":"bHMK"}}

```

Bytes typed by the user, base64. **No reply on success** — see
[Silent commands](#silent-commands). A failure answers `Error`.

### `TerminalResize`

```json
{"type":"TerminalResize","data":{"id":1,"size":{"cols":120,"rows":40}}}

```

Signals the process group, so applications that ask the kernel for the window
size (`vim`, `htop`) redraw. **No reply on success.** A failure answers `Error`.

### `TerminalClose`

```json
{"type":"TerminalClose","data":{"id":1}}

```

Kills the session's process group and frees its slot. **No reply on success.**
Sending input for a closed session answers `Error`.

Output that was already in flight may still arrive after this. A client should
ignore `TerminalOutput` for an id it has closed, and must not assume that closing
silences the session instantly.

### `TerminalOutput` (unsolicited)

```json
{"type":"TerminalOutput","data":{"id":1,"bytes":"G1sxOzMybWhpDQo="}}

```

Output from the program, base64. Not a reply to anything.

### `TerminalExited`

```json
{"type":"TerminalExited","data":{"id":1,"status":0}}

```

The program ended and no further output follows. `status` is its exit code, or
`-1` when no status could be collected. The daemon also sends this when the
terminal fails. After it, the session id is no longer valid.

### Silent commands

`TerminalInput`, `TerminalResize` and `TerminalClose` do **not** reply when they
succeed. Only failures produce a frame.

This is deliberate. Responses carry no request id, and terminal output is
unsolicited, so an unconditional acknowledgement could not be matched to its
request: a client reading frames in order would mistake an `Ok` for an answer it
was waiting for, or vice versa. Silence also avoids doubling the traffic of a
typing session with a 40-byte frame per keystroke.

The consequence for a client is that it cannot wait for an acknowledgement. It
writes and continues, and treats an `Error` as a signal that something went
wrong. `TerminalOpen` is the only terminal request with a required reply, because
the client needs the session `id`.

### Errors

Requests for a session that is not open on this connection answer `Error` with a
`detail` naming the reason. A daemon may cap the number of concurrent sessions;
when the cap is reached, `TerminalOpen` answers `Error` rather than queueing.
