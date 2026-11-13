# 25. Reverse proxy

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**.
> Start conditions, prerequisites and sequencing: [§21](21-next-version-apps.md).

Bundle `org.anthracite.proxy`, package `anthracite:proxy@1.0.0`, service class
`socket`, one `panel.view`.

## 1. Scope

- Listeners: HTTP and HTTPS, configurable bind addresses.
- Virtual hosts by server name (SNI for TLS), with a default host.
- Routes by path prefix or exact match, to an upstream.
- Upstreams: `http://host:port` and `https://host:port` with verification. Unix
  socket upstreams are a later phase with an approved allowlist. No
  client-supplied upstream.
- TLS termination with certificates from the certificate store's published
  directory; HTTP to HTTPS redirect; HSTS opt-in.
- ACME HTTP-01: serve `/.well-known/acme-challenge/*` from
  `{shared}/acme/http-01` for hosts that enable it.
- WebSocket upgrade and HTTP/2 (ALPN).
- Header policy: strip hop-by-hop, set `X-Forwarded-*` only when configured,
  preserve cookies and `Origin`.
- Atomic `apply`: validate the whole document, keep the last good config, reload
  without dropping in-flight requests.
- Access log as a stream; listener status and the last reload error in `status`.

**Non-goals:** a forward proxy, CONNECT, a WAF, a CDN, caching, dynamic service
discovery, nginx config import, L4 TCP/UDP proxying or request transformation
beyond headers and redirects.

## 2. Why it is a service and not the engine router

The engine router is the management API: authenticated, origin-checked, mounted
under `/api/apps/<id>/`. Customer traffic has different hostnames, TLS, cookies
and volume. Making the engine a data-plane proxy would put feature logic and
traffic policy in the platform, and would make "the engine serves no UI and no
site" false. The proxy is a normal service that happens to own a socket; the
engine does not know it exists beyond the manifest.

Its service block shows the authority the operator approves:

```toml
manifest_version = 4

[service]
path = "bin/proxy"
class = "socket"
argv = [
  "--socket", "{socket}",
  "--data", "{data}",
  "--tls-dir", "{shared}/tls",
  "--acme-root", "{shared}/acme/http-01",
  "--http", ":8080",
  "--https", ":8443",
]
listen = ["tcp:8080", "tcp:8443"]
egress = ["any"]
```

The `{shared}` paths are the only cross-service channel; the engine sees the
ports and the egress class, not a feature name.

## 3. Configuration model

One document, versioned, applied atomically. These records are the `admin`
interface's types; [§23](23-certificate-manager.md) shows the functions.

```wit
record config {
  version: u64,
  listeners: list<listener>,
  upstreams: list<upstream>,
  hosts: list<host>,
}

record listener {
  id: string,
  bind: string,                 // ":8080", "127.0.0.1:8443"
  tls: bool,
  default-host: option<string>,
}

record upstream {
  id: string,
  url: string,                  // "http://127.0.0.1:3000", "https://app:8443"
  connect-timeout-ms: u32,
  read-timeout-ms: u32,
  tls-verify: bool,
}

record header { name: string, value: string }

record host {
  id: string,
  names: list<string>,
  certificate: option<string>,  // certificate id in {shared}/tls
  acme: bool,                   // serve /.well-known/acme-challenge from {shared}
  routes: list<route>,
}

record route {
  id: string,
  match: string,                // "/" or "= /health"
  upstream: option<string>,
  strip-prefix: bool,
  websocket: bool,
  redirect: option<string>,     // exactly one of upstream or redirect
  headers: list<header>,
}
```

`apply(config, expected-version)` validates names, references, duplicate hosts,
TLS material, timeouts, path syntax and that each route has exactly one of
`upstream` or `redirect`. A failure leaves the previous config running and
returns a typed error with the field path. A success persists to
`{data}/config.json` atomically and returns the new version.

The private key path is not in the document: `certificate` is an id, and the
service resolves `{shared}/tls/<id>/`. The argv carries `--tls-dir {shared}/tls`
and `--acme-root {shared}/acme/http-01`, frozen in the grant. An unknown id is a
validation error.

## 4. WIT outline

```wit
package anthracite:proxy@1.0.0;

interface admin {
  use anth:error/kinds@1.0.0.{error};

  record proxy-status {
    running: bool,
    listeners: list<string>,
    last-reload: u64,
    last-error: option<string>,
    version: u64,
  }

  record access-event {
    at: u64,
    host: string,
    method: string,
    path: string,
    status: u16,
    bytes: u64,
    duration-ms: u64,
    upstream: string,
  }

  snapshot: func() -> config;
  apply: func(config: config, expected-version: u64) -> result<config, error>;
  status: func() -> proxy-status;
  logs: func() -> result<stream<access-event>, error>;
  certificates: func() -> list<string>;
  reload-certificates: func() -> result<proxy-status, error>;
}
```

`reload-certificates` re-reads the published directory. The service also watches
it on an interval, so a certificate published by the certificate service is
picked up without a panel action; an invalid file keeps the last good resolver
and reports the error.

`config`, `listener`, `upstream`, `host`, `route` and `header` are the records in
[§22](22-platform-prerequisites.md); they are part of this interface.

## 5. Routes, CLI and UI

| Surface | Shape |
| --- | --- |
| `GET /config` | `snapshot` |
| `PUT /config` | `apply` with `expected-version` in the body |
| `GET /status` | `status` |
| `GET /logs` | `logs`, `stream = true`, `events = true` |
| `GET /certificates` | `certificates` |
| `POST /certificates/reload` | `reload-certificates` |
| CLI `proxy status`, `hosts`, `routes`, `reload` | read and reload; the document is edited in the panel |

The view has four sections: listeners, hosts/routes, upstreams and logs. The
editor is a form, not a raw config file. A certificate picker lists ids from the
proxy's own `certificates` call, which scans `{shared}/tls`; the view never calls
another app. A "test upstream" action checks the target from the service, not
from the browser. The FAB "Add host" opens a modal; a second action applies the
draft.

## 6. Security rules

- **Not an open proxy.** No absolute-form requests, no `CONNECT`, no upstream
  from a request header. An unmatched `Host` gets `421` or the default host,
  never a guess.
- **No engine socket exposure.** The engine's Unix socket is not a valid
  upstream. A loopback TCP upstream to the panel is explicit and shown at
  approval.
- **Timeouts and limits.** Header size, body size per route, connect/read
  timeouts and a maximum in-flight request count. A slow upstream must not
  exhaust the service.
- **TLS defaults.** Rustls defaults, TLS 1.2+, no insecure ciphers, SNI required
  for multi-host listeners, no fallback to a mismatched certificate.
- **Forwarded headers.** `X-Forwarded-For/Proto/Host` only when the listener is
  trusted; the engine's origin check already treats a forwarded host from a
  loopback peer specially (D-Origin), and the proxy must not forge it for remote
  clients by default.
- **Access log privacy.** The query string is logged only when configured;
  cookies and authorization headers are never logged. The ACME challenge path is
  served before redirects, needs no session, and is not a route to an upstream.
- **Challenge path.** A host with `acme = true` serves only the token file it
  finds under `{shared}/acme/http-01`; a missing or malformed token is `404`,
  never a directory listing.

## 7. Privileged ports

Default binds are `:8080` and `:8443` so a developer can run the stack without
root. Production uses one of:

- systemd socket activation, with the service inheriting the listening sockets;
- `CAP_NET_BIND_SERVICE` on the service binary;
- an operator-managed port forward.

The service reports a bind failure as a typed error and keeps the previous
listener; it does not retry in a tight loop.

## 8. Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| P1 | Config document, HTTP listener, host/path routing, upstreams, status | [§22 §2](22-platform-prerequisites.md#2-listener-and-egress-intent-in-the-manifest) |
| P2 | TLS from `{shared}/tls`, SNI, redirect, certificate reload | P1, [§23 C2](23-certificate-manager.md#5-dependencies-and-phases) |
| P3 | WebSocket, HTTP/2, header policy, body limits | P2 |
| P4 | Access log stream, counters, test upstream, atomic apply/rollback | P3, [§22 §4](22-platform-prerequisites.md#4-streams-quotas-and-backpressure) |
| P5 | Privileged-port deployment, sandbox caps, hardening | P4, [§22 §5](22-platform-prerequisites.md#5-system-tier-and-service-sandbox) |

## 9. Risks

- **Config blast radius.** One bad host can blackhole traffic. Whole-document
  validation and last-good reload are the answer; the panel shows the last error.
- **Port conflicts.** Another daemon may own 80/443. The service fails that
  listener, keeps the other, and says which.
- **Certificate reload.** A partially written file is ignored; the publisher uses
  atomic rename. The proxy never serves a certificate it could not parse.
- **Streaming bodies.** Large uploads through the proxy are bounded per route;
  the panel's own upload path is not the proxy's.
- **SSRF.** The proxy forwards only to configured upstreams, but a misconfigured
  upstream to a metadata address is still an operator error. The UI warns on
  link-local and metadata ranges.

## 10. Acceptance

- A host routes to an HTTP upstream, a second host terminates TLS with a
  published certificate, and HTTP redirects to HTTPS.
- A WebSocket echo works through the proxy.
- An invalid config is rejected with a field path and the previous config keeps
  serving.
- A certificate published while the proxy runs is picked up within the reload
  interval; an invalid one is ignored with a status error.
- The access log stream shows one event per request and survives a slow consumer
  within budget.
- The proxy refuses an absolute-form request and an unconfigured `Host`.

---
