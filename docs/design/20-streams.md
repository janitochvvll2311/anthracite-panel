# 20. Streams

> Part of the [design index](../../DESIGN.md). Status: implemented in v0.6.0
> (PRs #133–#141). The checkpoint before this work is `checkpoint/pre-streams`;
> the Terminal is the first real consumer.

## 1. Shape

Streams and futures remain WIT **values**, not async functions (§7.15):

```wit
follow: func(path: string) -> result<stream<list<u8>>, error>;
```

The function is synchronous and returns a handle. A consumer reads it; a producer
gets a writer. `async func` stays out of the public contract because it would make
asynchrony contagious through the router.

The HTTP mapping is by element type:

| Result | Response |
| --- | --- |
| `stream<list<u8>>` | `application/octet-stream`, chunked |
| `stream<record>` | `text/event-stream`, one JSON object per event |
| `future<T>` | `200` with `T` once it resolves |

A raw WebSocket is **not** a second app API. A route may declare
`transport = "websocket"`; the engine upgrades that route and maps the same WIT
stream to binary frames. The app still exposes `stream<T>`, and the generated
clients still speak the HTTP chunked/SSE shape by default.

## 2. Route opt-in

A manifest route may declare `stream = true`:

```toml
[[route]]
method = "GET"
path = "/stream"
call = "anthracite:terminal/subscription@1.0.0#follow"
stream = true
```

The engine validates that the called function exists and that its result is a
stream. A route without `stream = true` keeps the existing JSON behavior, so v2
apps and existing v3 apps do not change.

`AppCaller` gains a stream operation alongside `call`:

```rust
fn stream<'a>(
    &'a self,
    app: &'a str,
    interface: &'a str,
    function: &'a str,
    input: Value,
) -> StreamFuture<'a>;
```

The API route dispatcher returns `Body::from_stream` instead of `Json` when the
route is a stream. The same session, origin, composition and grant checks apply.

### 2.1 WebSocket transport

A stream route may opt into a WebSocket upgrade and name the call that receives
client frames:

```toml
[[route]]
method = "GET"
path = "/socket"
call = "anthracite:terminal/subscription@1.0.0#follow"
stream = true
transport = "websocket"
write = "anthracite:terminal/terminal@1.0.0#write"
```

- The handshake is a `GET` and the route must be a stream. The same session,
  origin, composition and grant checks run before the upgrade.
- Downstream, one WIT stream chunk becomes one binary message. The engine polls
  the next chunk only after `send` completes, so the socket carries backpressure.
- Upstream, a binary frame becomes a `call` to `write` with the query input plus
  `data: list<u8>`. The app still exposes WIT; this is a transport, not a second
  API.
- `events = true` is an HTTP-only alternative for `stream<record>`. A WebSocket
  route carries raw bytes.
- A failed stream closes with code 1011; a clean end closes normally.

### 2.2 SSE events

A `stream<record>` route sets `events = true`:

```toml
[[route]]
method = "GET"
path = "/watch"
call = "anthracite:home/host@1.0.0#watch"
stream = true
events = true
```

- Each WIT stream element is one complete JSON value; the engine prefixes it with
  `data: ` and terminates the event with a blank line.
- The response is `text/event-stream` and asks proxies not to buffer.
- A byte stream (`stream<list<u8>>`) must not set `events`; a record stream must.
- WebSocket routes carry bytes, so `events` is HTTP-only.

## 3. Service boundary

Native services keep WIT-RPC for ordinary calls and add a private streaming
endpoint:

```http
POST /__anth/stream
Content-Type: application/json

{"interface":"...","function":"...","input":{}}
```

- A failure before the first byte is the normal JSON error envelope with its
  status.
- A `200` response is chunked bytes. The producer closes the body on a clean EOF.
- An error after the first byte ends the body; the engine records the stream as
  truncated and closes the panel stream. A truncation is not silent: the panel
  observes EOF and shows the ended/degraded state.
- `Handler` gets a `stream` method with a default `not_found`, so existing
  services compile unchanged.
- The service reader applies backpressure: it only reads a new PTY chunk after the
  previous chunk has been written to the socket.

The terminal service implements this against its PTY output queue. `close`,
half-close and cancellation end the stream and release the session.

## 4. Panel bridge

The app frame's buffered fetch RPC cannot carry a live stream. The panel bridge
adds a streaming request:

- `anth:fetch` starts the request; a live response sends `anth:stream-head`, one
  `anth:stream-chunk` per `anth:stream-pull`, and then `anth:stream-end`.
- The frame exposes a real `ReadableStream` through the `Response.body` of
  `context.fetch`; existing `text()`/`json()` callers keep working by buffering.
- The frame pulls one chunk at a time and the panel reads only on that pull, so
  an app that stops reading stops the engine and the service.
- Cancelling the reader aborts the panel fetch and therefore the engine response.
- `context.socket(route, handlers)` opens a declared WebSocket route. The panel
  owns the socket (cookies and origin checks), validates the same prefix, and
  relays binary frames in both directions.

The bridge applies the same scoped URL check as `anth:fetch`: a module can only
stream from its own `/api/apps/{id}/` prefix.

## 5. Terminal

A second interface in the same package keeps the existing RPC id intact:

```wit
package anthracite:terminal@1.0.0;

interface subscription {
    use anth:error/kinds@1.0.0.{error};
    follow: func(id: string, seq: u64) -> result<stream<list<u8>>, error>;
}

world app {
    export terminal;
    export subscription;
}
```

The package version stays at 1.0.0. WIT has one version per package, so
bumping it to add an interface would also rename `terminal@1.0.0`, even though
that interface did not change. The new interface is additive and carries the
same version. It is named `subscription` because `stream` is a WIT keyword.

- The manifest adds the `subscription` export and a `GET /stream` route with
  `stream = true`.
- The native Terminal and the demo fixture both implement `follow` over their
  chunk queues.
- The frontend replaces the `/read` poll with `/stream` and consumes it through
  `fetch(...).body.getReader()`.
- `/read` stays for the CLI and older clients during the transition.

## 6. Backpressure, cancellation and quotas

- The service produces only as fast as the engine reads; the engine only reads as
  fast as the panel consumes.
- Cancelling the reader cancels the engine request and the service stream; the
  terminal service keeps the session alive unless `close` is called.
- A clean EOF means the producer finished; a truncated body means failure.
- Per-stream counters enforce `bytes-total` and `bytes-per-sec` from §7.7. The
  first implementation uses the existing constants (`8 MiB` total, `1 MiB/s`)
  and returns `too-large`/`exhausted` before the first byte where possible.

## 7. Phases

Each phase landed as a separate PR against `v0.6.0`:

1. **Contract and engine plumbing.** `stream = true`, `AppCaller::stream`, the
   chunked API response and a fake streaming app in engine tests.
2. **Service SDK + fixture.** `/__anth/stream`, `Handler::stream`, a fixture that
   emits deterministic chunks, and the service caller.
3. **Panel bridge.** Streaming fetch messages, `Response.body`, cancellation and
   buffered-fetch compatibility.
4. **Terminal.** `terminal/stream@1.0.0`, native PTY and demo implementations,
   `/stream` frontend, and the `/read` fallback.
5. **Quotas and conformance.** Backpressure, cancellation, half-close,
   error-after-partial-data, byte caps and the corresponding tests.
6. **WebSocket transport.** `transport = "websocket"`, the engine upgrade, the
   panel relay and Terminal input frames.
7. **Typed events / SSE.** `events = true` maps `stream<record>` to
   `text/event-stream`; Host Info `watch` is the first consumer.

Blobs build on this phase but stay a separate PR.

## 8. Out of scope

- Raw WebSockets as a second app API (only a declared stream transport).
- Async WIT functions.
- Remote panel and multi-operator stream ownership.
- Binary transfer quotas beyond the byte counters above.
