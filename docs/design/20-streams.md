# 20. Streams

> Part of the [design index](../../DESIGN.md). Status: M5/Phase 8 implementation
> plan. The checkpoint before this work is `checkpoint/pre-streams`; the Terminal
> is the first real consumer.

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

A raw WebSocket is **not** a second public API. WebSockets may be used behind the
service boundary if a transport needs them, but the panel and generated clients
consume the HTTP chunked/SSE shape.

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

- `anth:stream` starts a fetch, `anth:stream-chunk` carries bytes, and
  `anth:stream-end` carries the status or error.
- The frame exposes a real `ReadableStream` through the `Response.body` of
  `context.fetch`; existing `text()`/`json()` callers keep working by buffering.
- Cancelling the reader aborts the panel fetch and therefore the engine response.

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

Each phase is a separate PR against `v0.6.0`:

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

Typed events (`subscribe() -> stream<event>`) and blobs build on this phase but
stay separate PRs.

## 8. Out of scope

- Raw WebSockets as a public API.
- Async WIT functions.
- Remote panel and multi-operator stream ownership.
- Binary transfer quotas beyond the byte counters above.
