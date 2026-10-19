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
{"type":"ready","shell":"/bin/sh","cols":120,"rows":40}
{"type":"output","data":"aGVsbG8K"}
{"type":"exit","code":0}
{"type":"error","detail":"too many terminal sessions"}
{"type":"pong"}
```

| Frame | Meaning |
| --- | --- |
| `ready` | Always the first frame. Shell program and actual size. |
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
