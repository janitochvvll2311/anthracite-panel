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

A line is a complete message. A message must not contain a raw newline; JSON
encoding never produces one, so `\n` is always a frame boundary.

## Request and response shape

Both messages are JSON objects with two members:

```json
{ "type": "<Variant>", "data": <payload> }
```

`type` names the operation and `data` carries its arguments. The `data` member
is omitted entirely for variants that take no arguments (`HostInfo`).

The pair is request/response: the client writes one request and reads one
response on the same connection. A connection may carry several request/response
pairs in sequence; the daemon serves them in order.

> Note: this envelope is specific to the socket transport. The HTTP API omits it
> and names the command in the route instead — see [api-endpoints.md](api-endpoints.md).

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
| Daemon goes away | The connection closes with no message; a write fails with `Broken pipe` |

Dropping the connection is the daemon's only way to end a conversation, so a
client should treat EOF as an error rather than an empty response.
