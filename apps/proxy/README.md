# Proxy

The edge of the composition: a native `socket`-class app that terminates TLS,
routes customer traffic by host and path and forwards it to configured
upstreams. Bundle `org.anthracite.proxy`, package `anthracite:proxy@1.0.0`, one
`panel.view` and one `panel.modal`.

## Authority

The manifest freezes what the operator approves:

```toml
[service]
class = "socket"
argv = ["--socket", "{socket}", "--data", "{data}", "--broker", "{broker}",
        "--shared", "{shared}", "--http", ":8080", "--https", ":8443"]
listen = ["tcp:8080", "tcp:8443"]
egress = ["any"]
```

- The two listeners are bound at start-up from the argv. The document may
  select them and map hosts onto them, but it **cannot open another port**; a
  bind that is not one of the argv endpoints is a validation error.
- `{shared}` is passed once because the manifest validator allows at most one
  occurrence. The process derives `{shared}/tls` (published certificates) and
  `{shared}/acme/http-01` (HTTP-01 tokens) from it.
- The engine sees `listen` and `egress` as intent; the data plane is entirely
  the service's. `egress = ["any"]` is what a proxy needs and it is visible at
  approval.

## The document

One versioned JSON document (`{data}/config.json`), applied atomically:

```json
{
  "version": 1,
  "listeners": [{ "id": "plain", "bind": ":8080", "tls": false,
                  "default-host": "site", "forwarded": false }],
  "upstreams": [{ "id": "app", "url": "http://127.0.0.1:3000",
                  "connect-timeout-ms": 2000, "read-timeout-ms": 5000,
                  "tls-verify": true }],
  "hosts": [{
    "id": "site", "names": ["example.com"],
    "certificate": "0123456789abcdef", "acme": true,
    "redirect-https": true, "hsts": false,
    "routes": [{ "id": "root", "match": "/", "upstream": "app",
                 "strip-prefix": false, "websocket": false,
                 "redirect": null, "headers": [], "body-limit": null }]
  }]
}
```

- `listeners[].bind` must be `:8080` (HTTP) or `:8443` (TLS) and `tls` must
  match the argv endpoint.
- `hosts[].certificate` is an id published under `{shared}/tls/<id>/` by the
  Certificates app. The proxy never reads another app's routes; it lists its
  own directory.
- `hosts[].redirect-https` and `hosts[].hsts` are opt-in. A host with no
  certificate cannot redirect.
- `routes[].match` is a prefix (`/`) or an exact match (`= /health`). Exact
  beats prefix, the longest prefix wins, and declaration order breaks a tie.
- Exactly one of `upstream` or `redirect` per route. `redirect` is a path or an
  absolute `http(s)` URL. `strip-prefix` only applies to prefix routes.
- `routes[].body-limit` is bytes (default 10 MiB, hard maximum 1 GiB).
- `listeners[].forwarded` gates `X-Forwarded-For/Proto/Host`: off by default,
  and inbound `X-Forwarded-*` are dropped when off.

`apply(config, expected-version)` validates the whole document (ids,
references, duplicate names, certificate ids, timeouts, path syntax, headers),
persists it atomically to `{data}/config.json` and swaps the running snapshot.
A discarded version, a malformed field or an unparseable certificate leaves the
previous document serving. The error message starts with the field path
(`hosts[0].routes[1].upstream: …`).

## Data plane

- Virtual hosts by the HTTP `Host` header (port and trailing dot stripped,
  case-insensitive). An unmatched name answers `421` unless the listener has a
  default host.
- ACME `/.well-known/acme-challenge/<token>` is served before redirects and
  routes, from `{shared}/acme/http-01`, for hosts with `acme = true`; only a
  plain token name is read, never a directory.
- HTTP to HTTPS redirect is a `308` to the configured TLS listener's port.
  HSTS is sent only on TLS and only when the host opts in.
- TLS is rustls with TLS 1.2+, ALPN `h2` then `http/1.1`, and SNI selecting the
  host certificate (the listener default is the fallback). An invalid published
  file keeps the last good resolver and appears in `status.last-error`.
- WebSocket upgrade tunnels only on routes with `websocket = true`; the 101
  keeps the upgrade headers and both sides are copied in both directions.
- Absolute-form requests, `CONNECT` and upgrades on non-WebSocket routes are
  refused. There is no upstream from a request header and no unix-socket
  upstream.
- Hop-by-hop headers are stripped in both directions (except the 101 upgrade
  pair); cookies and `Origin` are preserved.
- Access log: one event per request on `GET /logs` (`text/event-stream` via the
  engine). The queue is bounded per viewer; a slow viewer drops events and
  `status.dropped` reports the count. Failed or timed-out TLS handshakes leave a
  rate-limited trace and `status.handshake-errors` counts them.

## Surface

| Route | Function |
| --- | --- |
| `GET /config` | `snapshot` |
| `PUT /config` | `apply` (`{config, expected-version}`) |
| `GET /status` | `status` |
| `GET /logs` | `logs` (stream, events) |
| `GET /certificates` | ids under `{shared}/tls` |
| `POST /certificates/reload` | re-read the published directory |
| `GET /hosts` / `GET /routes` | the document's views |
| `POST /upstreams/test` | `{id}`; a probe from the service, not the browser |

CLI: `anth app proxy status|hosts|routes|reload`.

The panel view has four sections (listeners, hosts & routes, upstreams, access
log). The FAB adds a host through the modal and applies the draft; the modal
hands the host to the view with `openView` params, so the edit stays a draft
until the operator applies it. Without a modal slot the view says so and the
document can be edited through the API.

## Build and test

```sh
apps/proxy/build.sh                     # frontend + service + bundle
(cd apps/proxy/backend && cargo test)   # unit + loopback e2e
apps/proxy/frontend/bun test            # SSE/apply helpers
apps/proxy/frontend/bun run typecheck
```

`cargo test` in the backend includes loopback tests for two hosts, route
precedence, apply/rollback and version conflicts, TLS/SNI/HTTP2, certificate
reload with an invalid file, a WebSocket echo and the access log stream. The
bundle is checked with:

```sh
./target/debug/anth-conformance apps/proxy/dist/proxy.anth
```

## Decisions

- Listener binds come from the grant, not the document (`docs/decisions`).
- `redirect-https`, `hsts`, `forwarded`, `body-limit` and `status.dropped`
  extend the design outline to cover the acceptance criteria without a second
  document.
- One `{shared}` in argv is the validator's limit; the two subdirectories are
  derived.
