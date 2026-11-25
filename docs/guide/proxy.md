# Reverse proxy

The proxy app is the edge data plane. It terminates HTTP and HTTPS, routes by
host and path, upgrades WebSockets, negotiates HTTP/2, and applies a whole
document at once with rollback.

Design: [§25](../design/25-reverse-proxy.md). Decisions: [ADR 0001](../decisions/0001-can-the-proxy-document-open-listeners.md),
[0002](../decisions/0002-how-does-the-document-express-https-redirect-hsts-forwarded-head.md).

## Approval

| Field | Value |
| --- | --- |
| App id | `org.anthracite.proxy` |
| Service class | `socket` |
| Listen | `tcp:8080`, `tcp:8443` |
| Egress | `any` (upstreams are chosen by the document) |
| Interface | `anthracite:proxy/admin@1.0.0` |

The document may map hosts onto the two frozen binds; it cannot invent a third
listener. TLS material is read from `{shared}/tls/<id>/`, written by the
certificate app. In production the release build uses `:80`/`:443` through
`CAP_NET_BIND_SERVICE`, granted by the supervisor; a developer build uses the
high ports. Socket activation is not used.

## Behaviour

- Whole-document `apply` with a version; a failed apply keeps the last good
  document and reports the error in `status`.
- Virtual hosts by server name (SNI for TLS), path-prefix or exact routes,
  `http` and `https` upstreams with verification.
- HTTP-01: `/.well-known/acme-challenge/*` is served from
  `{shared}/acme/http-01` for hosts with `acme = true`.
- WebSocket upgrade, HTTP/2 over ALPN, optional HTTP→HTTPS redirect and HSTS.
- Header policy strips hop-by-hop headers and sets `X-Forwarded-*` only when the
  listener opts in; cookies and `Origin` are preserved.
- The access log is a stream with a byte budget; under budget pressure it is the
  one lossy stream and reports the dropped count.
- Removing the app stops the listeners. Already-published certificates remain
  and ACME HTTP-01 fails its self-check; DNS-01 still works, and containers and
  the store are unaffected.

## Routes and CLI

| Route | CLI | Function |
| --- | --- | --- |
| `GET /config` | — | current document |
| `PUT /config` | — | validate, apply and version |
| `GET /status` | `anth status` | listeners, last reload, counters |
| `GET /logs` | — | access log stream |
| `GET /certificates` | — | published certificates seen |
| `POST /certificates/reload` | `anth reload` | reload TLS without a restart |
| `GET /hosts` | `anth hosts` | host list |
| `GET /routes` | `anth routes` | route list |
| `POST /upstreams/test` | — | probe an upstream |

## Tests

`apps/proxy/backend` covers route precedence, host/SNI parsing, apply versions
and rollback, header policy and the log budget. `tests/e2e/src/specs/proxy.spec.ts`
drives two loopback upstreams, TLS from a fixture certificate, a WebSocket echo
and an HTTP/2 request against a running panel.
