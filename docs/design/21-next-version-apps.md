# 21. The next version: business apps

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**, the
> milestone after [v0.6.0](18-v0.6-plan.md).

Status: **draft for review**. The scope is proposed; the mechanisms marked
*proposed* are not binding until accepted in [§16](16-decision-log.md). Nothing
here starts before the v0.6.0 entry conditions in §2 are met.

v0.6.0 proves the platform with system apps and third-party apps. v0.7.0 uses
that proof to run a real edge: TLS material, certificates issued by an ACME CA,
one ingress in front of the workloads, containers behind it, and a store that
installs apps without giving an app the operator's authority.

The five apps are separate bundles. Each is installable and removable on its own;
ACME imports the certificate store and the proxy consumes its published files,
but neither is compiled into the other. None of the five requires an engine edit;
each exposes WIT through routes, the CLI and the panel exactly like the system
apps. The engine does not learn what a certificate, a virtual host, a container
or a store is. The platform work they do need — the service broker, listener
approval, secret custody — is generic and listed in §3; it is not feature logic
in the engine.

---

## 1. Why these five

The system apps answer "what is on this host?". These answer "what does this host
serve, and to whom?".

| App | Bundle id | The pain it removes | The business value |
| --- | --- | --- | --- |
| Certificate Manager | `org.anthracite.certs` | Certificates and keys scattered across `/etc`, expiry found by outage | One inventory, expiry and chain status, audited import, keys never rendered |
| ACME client | `org.anthracite.acme` | Manual renewals, one vendor's client, wildcard certificates out of reach | Zero-touch issuance and renewal against any RFC 8555 CA, HTTP-01 or DNS-01 |
| Reverse Proxy | `org.anthracite.proxy` | Hand-edited nginx, TLS and routing in a text file nobody reviews | Typed host/path routing, TLS termination, WebSockets, atomic apply and logs |
| Container Manager | `org.anthracite.containers` | SSH, `docker` on the command line, logs copied by hand | Workload inventory, lifecycle, logs, stats and exec behind the same approval |
| App Store | `org.anthracite.store` | Copying zips, trusting a publisher by habit, silent updates | Signed catalogues, digest-pinned bundles, explicit install and re-approval |

The buyer is the same in all five cases: a self-hoster, a small IT team or an MSP
running a handful of hosts. They do not buy "a panel"; they buy not being paged
at 02:00 because a certificate expired, and not giving every operator a root
shell to restart a container.

Adjacent tools already exist: certbot, lego, nginx, Caddy, Portainer, Watchtower.
The bet is not a better ACME client or a faster proxy; it is one composition, one
approval and one audit across them, and an app model that can replace any of the
five without a platform release. The lock-in is the contract, not a vendor: any
RFC 8555 CA, Docker or Podman, and any signed index.

The platform argument is the differentiator. The store cannot approve an app,
the proxy sees only the material the certificate manager publishes, the ACME
client cannot write the system trust store, and the container manager cannot
mount the host filesystem. Each has exactly the authority its interface declares,
and the operator sees the class, the argv and the ports at approval.

### 1.1 What the five share

- **They are service bundles with a UI slot.** Each has a service half for host
  reach and a `panel.view` module under `web/`; WIT stays the public API (§4 of
  [§19](19-v0.6-baseline.md#4-service-lifecycle-and-wit-rpc)).
- **They have a management plane and a data plane.** The management plane is
  WIT over `POST /__anth/call`, wrapped by routes, CLI and the panel. The data
  plane is native: the proxy serves HTTP, the container manager talks to the
  runtime and bridges logs, the ACME client speaks TLS to the CA. The engine
  never proxies customer traffic.
- **They hold secrets.** Account keys, EAB secrets and DNS tokens live in the
  service's `{data}` directory (`0600`). Private keys live in the certificate
  manager's `{data}` and are published to `{shared}` only for the services that
  need them. None of it is `anth:store/kv` state, and none of it is read back
  through a route, a log line or the panel. A manual certificate import is the
  one explicit write path, and the UI calls it out.
- **They stream.** Order progress, proxy access logs, container logs and stats,
  and bundle downloads are `stream<T>` under the rules of
  [§20](20-streams.md). A poll-only version is a fallback, not the design.
- **They are system-tier in a release.** Their class is `socket`, `admin` or
  `host`; they need the distribution key and the service sandbox before an
  operator can call the deployment supported (§3.5).

---

## 2. Entry conditions

v0.7.0 starts when v0.6.0 is tagged and these are true.

### 2.1 v0.6.0 is complete

- M4 closes with the golden system composition and the proof that removing `net`
  leaves a working system ([§18](18-v0.6-plan.md)).
- Typed events and stream quotas are implemented ([§20](20-streams.md)).
- The conformance kit, `fetch-wit`, the audit/diff UI and service sandbox are
  shipped ([§18](18-v0.6-plan.md), phase 9).
- A distribution key signs the system tier, or v0.7.0 is explicitly a debug-only
  preview and says so in the release notes.

### 2.2 The platform prerequisites in §3

These are not "nice to have": without them, one of the five apps either cannot
be written without ambient authority or cannot be shipped honestly.

### 2.3 The scope is frozen

The five apps are the milestone. A sixth app, a hosted service, a payment flow or
a Kubernetes integration is a different document and a different approval.

---

## 3. Platform prerequisites

These land in v0.7.0 phase 0 unless the table says otherwise. Each is generic
mechanism: none mentions a certificate, a container or a store.

### 3.1 Service call broker

A service must be able to call another service's interface through the grant,
not through the public router.

- **Today:** a route is the only door into an app (D-Cli-Invocation), and a
  service sees `interface`, `function` and `input`, never `/api/apps/...`.
- **Needed:** a private broker socket, handed to the service as `{broker}` (a new
  v4 placeholder beside `{socket}`), and a generated service client. The broker
  resolves the interface to the composed provider, applies the same grant and
  budget checks as `AppCaller`, and audits the call. It is the service half of
  the shim in [§19 §10](19-v0.6-baseline.md#10-adapter-and-shim).
- **Why it gates the plan:** ACME imports the certificate store; a future
  DNS-01 provider bundle would be the second typed consumer. A file drop or a
  panel courier would put private keys or operator authority in the wrong
  process.
- **Fallback if the broker slips:** ACME writes an order directory under
  `{shared}/tls/incoming/` with a typed `meta.json` and `0600` material, and the
  certificate manager ingests it. This is a contingency, not the plan; it is
  audited as a degraded path and removed when the broker lands.

### 3.2 Listener and egress intent in the manifest

The operator must see what a service intends to bind and where it intends to
connect before approving it. `argv` shows flags, but a flag is a convention, not
a contract.

Proposed manifest v4: optional `[service]` keys, with v3 still accepted.

```toml
[service]
path = "bin/proxy"
sha256 = "…"
class = "socket"
argv = ["--socket", "{socket}", "--data", "{data}", "--http", ":8080", "--https", ":8443"]
listen = ["tcp:8080", "tcp:8443"]
egress = ["any"]                    # or ["host:443", "10.0.0.0/8:5432"]
```

- `listen` and `egress` are **intent shown at approval and frozen in the grant**.
  They are not enforced until the sandbox phase; `argv` remains the executable
  truth.
- The engine refuses a declared listener that is not a `tcp:` or `udp:` address.
  It does not allocate ports, and it does not reserve them.
- In a release, the sandbox maps `listen` to `CAP_NET_BIND_SERVICE` or socket
  activation and `egress` to a default-deny policy. In a debug build the class
  and the argv are the audit trail.
- A service that declares neither is unchanged (v3 semantics).

### 3.3 Secret custody

- Service secrets live in `{data}` (`0700` directory, `0600` files), are read
  only by the service, and are never returned by a WIT function or a route.
- The panel may **write** a secret (a DNS token, an EAB secret, a certificate
  key on manual import) but never read it back. A write-only form is enough;
  "show token" is not a feature we add.
- `anth:store/kv` stays a plaintext small-state map and is not used for secrets.
  If a wasm app ever needs a secret, that is a separate `anth:store/secret`
  capability with its own risk; it is not in v0.7.0.
- Logs and audit records carry identifiers and fingerprints, never material: no
  PEM, no token, no `Authorization` header, no container environment value.

### 3.4 Streams, quotas and backpressure

The existing stream rules apply. The new consumers:

| Stream | Shape | Budget |
| --- | --- | --- |
| ACME order progress | `stream<order-event>` | small, per order, capped events |
| Proxy access log | `stream<access-event>` | per second and total; lossy, with a dropped counter |
| Container logs | `stream<list<u8>>` | tail + follow, total cap, idle timeout |
| Container stats | `stream<sample>` | one sample per interval, capped rate |
| Store download | `stream<list<u8>>` | bundle cap (32 MiB default), digest verified |

A stream that enforces a hard budget ends with `exhausted`, not a silent
truncation. The access log is the one lossy stream: it drops the oldest events
and reports the count, because stopping the log would stop the proxy's
visibility. The panel renders the reason either way.

### 3.5 System tier and service sandbox

`class` is risk today; the child still runs as the engine user
([§19 §4](19-v0.6-baseline.md#4-service-lifecycle-and-wit-rpc)). A proxy that
binds `:443`, a certificate manager that reads `/etc/ssl` and a container manager
that holds the runtime socket are only supportable when:

- the bundle is signed by the distribution key (`system` tier), and
- the service runs with a privilege drop, a namespace or Landlock/seccomp
  profile, and the port/capability set it declared.

Until then v0.7.0 is a debug preview. That is an honest release note, not a
limitation to bury.

### 3.6 Surface exports

The panel manager must discover providers without hard-coding an app id. Add the
composed app's exported interfaces to `SurfaceApp` in
`GET /api/engine/composition`:

```json
{ "id": "org.anthracite.store", "exports": ["anthracite:store/catalog@1.0.0"], "…": "…" }
```

This is generic surface data. The manager can then find the store provider, the
certificate provider or any future provider by interface, not by name. The
composition still binds one provider per interface id.

### 3.7 A service audit bridge

Container start/stop/exec, certificate import and proxy apply are operator
actions that must appear in the engine audit (D-Audit), next to installs and
approvals.

Proposed: an `anth:audit/record@1.0.0` host call available to services through
the broker, write-only, with `action`, `result` and a sanitized `detail`. The
engine stamps `actor`, `app` and time. If this slips, each service writes a local
JSONL audit in `{data}/audit.jsonl` and the panel shows it; the release notes say
the records are app-local, not engine-global.

### 3.8 Time and scheduling

There is no engine cron. A service owns its timers:

- ACME renewal and store refresh run as a loop inside the service, with jitter,
  bounded backoff and persisted state, so a restart resumes rather than restarts
  the schedule.
- Wall-clock correctness matters for certificate expiry and ACME. The service
  reports clock skew in `status`; a large skew is a warning, not a reason to
  silently retry faster.

---

## 4. Certificate Manager

Bundle `org.anthracite.certs`, package `anthracite:certs@1.0.0`, service class
`admin`, one `panel.view`.

### 4.1 Scope

- Inventory certificates from approved roots: `/etc/ssl/certs`,
  `/etc/letsencrypt/live`, a service import directory, and the managed store.
- Show names, issuer, validity, key type, fingerprint and chain state.
- Import a PEM chain and key, validate that they match, and publish them for
  consumers.
- Remove a managed certificate, with a confirmation.
- Mark a managed certificate `auto-renew`; the ACME app reads that flag. Only
  `source = acme` certificates can be marked, so a manual import is never
  renewed behind the operator's back.
- Watch expiry and surface `valid`, `expiring` (30/14/7 days), `expired`,
  `invalid`.

**Non-goals:** being a CA, issuing internal certificates, modifying the system
trust store, OCSP/CRL infrastructure, key escrow, HSM support, or showing a
private key anywhere in the panel.

### 4.2 Authority and custody

- Class `admin`; the roots are literal `--scan` argv entries and the publish
  directory is `--publish {shared}/tls`, all frozen in the grant and visible at
  approval.
- Writes go to `{data}/certs/<id>/` (`0700`) and are published atomically to
  `{shared}/tls/<id>/`:

  ```
  {shared}/tls/<id>/fullchain.pem   0600
  {shared}/tls/<id>/privkey.pem     0600
  {shared}/tls/<id>/meta.json       0600
  ```

- `{shared}` is the service trust domain. An app that is granted `{shared}` can
  read the published material, so the grant is shown as a service-to-service
  boundary, not as a convenience.
- The private key is never returned by the service through WIT or HTTP. It
  enters through an explicit import or an ACME issuance; the only consumer path
  is the published directory, read by the proxy service.
- A system certificate is **inventory only** unless the operator imports a copy.
  The manager does not copy keys out of `/etc/letsencrypt` behind the operator's
  back.

### 4.3 WIT outline

```wit
package anthracite:certs@1.0.0;

interface store {
  use anth:error/kinds@1.0.0.{error};

  enum source { acme, imported, system }
  enum status { valid, expiring, expired, invalid }

  record certificate {
    id: string,
    names: list<string>,
    issuer: string,
    not-before: u64,
    not-after: u64,
    key-type: string,
    source: source,
    fingerprint-sha256: string,
    status: status,
    auto-renew: bool,
  }

  record import-request {
    cert-pem: string,
    chain-pem: string,
    key-pem: string,
    names: list<string>,
    auto-renew: bool,
  }

  record validation {
    ok: bool,
    reason: option<string>,
    chain-ok: bool,
    key-matches: bool,
  }

  inventory: func() -> list<certificate>;
  get-certificate: func(id: string) -> option<certificate>;
  import-certificate: func(request: import-request) -> result<certificate, error>;
  store-issued: func(request: import-request) -> result<certificate, error>;
  remove-certificate: func(id: string, acknowledge: bool) -> result<bool, error>;
  validate: func(id: string) -> validation;
  publish: func(id: string) -> result<certificate, error>;
  set-auto-renew: func(id: string, enabled: bool) -> result<certificate, error>;
}
```

`error` is the contract error from `anth:error`, mapped to the envelope by the
service SDK. `import-certificate` is the operator import and sets
`source = imported`; `store-issued` is the ACME entry point and sets
`source = acme`. The service owns the source, not the caller. Both are audited
through the service audit bridge (§3.7), and the request body is never logged.

### 4.4 Routes, CLI and UI

| Surface | Shape |
| --- | --- |
| `GET /certs` | `inventory` |
| `GET /certificate?id=…` | `get-certificate` |
| `POST /certs` | `import-certificate` |
| `DELETE /certificate?id=…&acknowledge=true` | `remove-certificate` |
| `POST /certificate/validate` | `validate` (body carries the id) |
| `POST /certificate/publish` | `publish` (body carries the id) |
| `POST /certificate/auto-renew` | `set-auto-renew` (body carries the id and flag) |
| CLI `certificates list`, `show`, `import`, `validate` | the same calls |

The CLI column is illustrative: every verb is a `[[cli]]` entry naming the same
`interface#function` as the route above it, so a verb without a route is
`unavailable` (D-Cli-Invocation).

The view is a table with status badges, expiry, source and a details modal. The FAB is icon-only: one action opens the import modal, whose file picker
uses `files = true`; the module reads the PEM text and calls its own service. A
second action refreshes the inventory. The modal warns that the key transits the
browser and recommends ACME or the service import directory for production.

### 4.5 Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| C1 | Inventory, parsing, validation, table, details | §3.2, §3.3 |
| C2 | Import, managed store, publish to `{shared}/tls` | C1, §3.3 |
| C3 | Auto-renew flag, expiry warnings, remove guards; consumer counts wait for a proxy-published `consumers.json` in `{shared}` (not a typed import) | C2 |
| C4 | System-root scan, audit bridge, release hardening | C3, §3.5, §3.7 |

### 4.6 Risks

- **Key in the browser.** Manual import means the key is in the page's memory.
  The UI says so. The preferred paths are ACME or dropping the pair into the
  service import directory.
- **Shared directory trust.** `{shared}` is not per-consumer. The grant text must
  say that publishing a key makes it readable by every service with `{shared}`.
- **System roots vary.** Distros disagree on `/etc/ssl/certs` and
  `/etc/letsencrypt`, and the engine user may not be able to read them. The
  defaults are scan roots, not promises; the operator edits argv or fixes the
  permission boundary. The service reports an unreadable root instead of
  pretending it is empty.
- **Remove while in use.** A removed certificate leaves the published files until
  the proxy reloads; the panel warns, and the proxy keeps its last-good config
  until the reload.

### 4.7 Acceptance

- A PEM pair imports, validates and appears in `{shared}/tls/<id>` with `0600`.
- A mismatched key/cert is refused with a typed error and no files written.
- The panel never displays the private key and no route returns it.
- Removing a certificate used by a proxy host requires acknowledgment; the
  proxy keeps the last-good configuration until it reloads.
- The service survives a malformed PEM, a missing root and a read-only `{shared}`
  without taking down the engine.

---

## 5. ACME client

Bundle `org.anthracite.acme`, package `anthracite:acme@1.0.0`, service class
`socket`, one `panel.view`.

### 5.1 Scope

- RFC 8555 accounts and orders against any ACME directory: Let's Encrypt,
  Buypass, ZeroSSL (with EAB), Google Trust Services, or an internal step-ca.
- Challenges: HTTP-01 and DNS-01. TLS-ALPN-01 is a later phase.
- Key types: ECDSA P-256 (default), RSA 2048.
- A worker issues, polls, finalizes and downloads; the UI shows a timeline.
- Renewal: read `auto-renew` certificates from the certificate store, renew at
  about two thirds of lifetime or when the ACME Renewal Information (ARI) says
  so, with backoff.
- Revocation with a reason code.
- Staging and production directories; never auto-promote a staging order.

**Non-goals:** being a CA, managing DNS zones, buying domains, TLS-ALPN-01 in
v1, wildcard issuance without DNS-01, or a general-purpose HTTP client for apps.

### 5.2 Authority and custody

- Class `socket`; outbound TLS to the CA on 443. The directory URLs come from a
  configured allowlist; `create-account` may name one of them, not an arbitrary
  URL. The service refuses loopback, link-local and cloud metadata addresses
  unless the operator opts in (an internal step-ca is the opt-in case).
- Account keys and EAB secrets live in `{data}/accounts/<id>/` at `0600`. They
  are never returned by a route and never logged.
- HTTP-01 does **not** bind port 80. The token directory is
  `--http01-root {shared}/acme/http-01` in argv; the client writes the key
  authorization to `{shared}/acme/http-01/<token>` (`0600`) and relies on the
  ingress that owns port 80 to serve `/.well-known/acme-challenge/<token>`. The
  proxy does this for a host with `acme = true`. Any other listener can read the
  same directory; this is the provider-agnostic seam.
- Before telling the CA the challenge is ready, the client fetches
  `http://<name>/.well-known/acme-challenge/<token>` and compares the body. A
  mismatch is a clear, early failure.
- DNS-01 uses one of three solver kinds, all configured in the service:
  `manual` (the operator copies the TXT record and confirms), `rfc2136` (generic
  dynamic DNS) and `webhook` (an operator-supplied HTTPS endpoint that presents
  and cleans up the record). Vendor-specific adapters are additive modules, not
  separate apps, because `anthracite:` is reserved for the system tier.
- Issued material goes to the certificate store through
  `anthracite:certs/store#store-issued` over the broker. The key is generated
  locally; the CA never sees it. If the store is absent, the order is
  `issued-unstored` and the material stays in `{data}` with a warning, not
  silently dropped.

### 5.3 WIT outline

```wit
package anthracite:acme@1.0.0;

interface client {
  use anth:error/kinds@1.0.0.{error};

  enum challenge { http-01, dns-01 }
  enum order-status { pending, ready, processing, valid, invalid, revoked }

  record account {
    id: string,
    directory: string,
    contact: list<string>,
    status: string,
    created-at: u64,
    key-type: string,
  }

  record eab-credentials { kid: string, hmac-key: string }

  record issue-request {
    account: string,
    names: list<string>,
    challenge: challenge,
    solver: option<string>,
    key-type: string,
    auto-renew: bool,
    staging: bool,
  }

  record order {
    id: string,
    account: string,
    names: list<string>,
    challenge: challenge,
    status: order-status,
    expires-at: u64,
    certificate: option<string>,
    last-error: option<string>,
  }

  enum event-kind { created, challenge-ready, validated, issued, failed, renewed, revoked }
  record order-event { order: string, kind: event-kind, at: u64, detail: option<string> }

  record capability { challenge: challenge, solvers: list<string>, directories: list<string> }

  list-accounts: func() -> list<account>;
  create-account: func(directory: string, contact: list<string>, key-type: string, eab: option<eab-credentials>) -> result<account, error>;
  issue: func(request: issue-request) -> result<order, error>;
  renew: func(certificate: string) -> result<order, error>;
  revoke: func(certificate: string, reason: string) -> result<bool, error>;
  list-orders: func() -> list<order>;
  follow: func(order: string) -> result<stream<order-event>, error>;
  capabilities: func() -> list<capability>;
}
```

`create-account` takes `eab.hmac-key` as a write-only secret and never returns it.
`issue` returns immediately; the worker owns the CA conversation. `follow` is
the SSE stream; a poll fallback is allowed for the CLI.

### 5.4 Routes, CLI and UI

| Surface | Shape |
| --- | --- |
| `GET /accounts`, `POST /accounts` | list and create |
| `GET /orders`, `POST /orders` | list and issue |
| `GET /capabilities` | `capabilities` |
| `GET /order/events?order=…` | `follow`, `stream = true`, `events = true` |
| `POST /order/renew` | renew (body carries the certificate id) |
| `POST /order/revoke` | revoke (body carries the certificate id and reason) |
| CLI `acme accounts`, `issue`, `orders`, `renew` | the same calls |

The view has two panes: accounts and orders. The FAB "Issue certificate" opens a
modal with names, CA directory, challenge, key type, staging and auto-renew. The
order timeline is a stream with a visible poll fallback. The details pane shows
the challenge type and the exact next step for manual DNS, never the account key.

### 5.5 Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| A1 | Account create, directory fetch, order state, staging | §3.1, §3.2, §3.3 |
| A2 | HTTP-01 via the shared token directory, issue, import into certs | A1, §4 C2, proxy data plane |
| A3 | Renewal worker, ARI, backoff, expiry read from certs | A2, §4 C3 |
| A4 | DNS-01 manual + RFC 2136 + webhook, wildcard | A2 |
| A5 | Revocation, audit, hardening | A4, §3.5, §3.7 |

### 5.6 Risks

- **Rate limits.** Let's Encrypt counts failed validations and duplicate
  certificates. The client uses staging by default in the UI, backs off on
  `Retry-After`, and refuses a duplicate order unless the operator asks.
- **Challenge routing.** If the ingress is down or the host does not resolve to
  it, HTTP-01 fails. The self-check turns a CA timeout into an actionable error.
- **Wildcards.** DNS-01 is the only path; the UI must not offer a wildcard with
  HTTP-01.
- **Clock and CA policy.** A skew or an unannounced CA change surfaces as a
  typed failure, not a retry storm.
- **Secret writes.** The account key and EAB secret are write-only. A lost key is
  a new account, not a recovery flow.

### 5.7 Acceptance

- A staging order against a local ACME test server completes via HTTP-01, and the
  issued chain is imported into the certificate store and published.
- The account key is not present in any route response, log or audit record.
- A failed challenge reports the CA error and the self-check result.
- Renewal runs on schedule, survives a restart, and never renews a certificate
  whose `auto-renew` is false.
- HTTP-01 works with the proxy and with a non-Anthracite listener that reads the
  same token directory.

---

## 6. Reverse Proxy

Bundle `org.anthracite.proxy`, package `anthracite:proxy@1.0.0`, service class
`socket`, one `panel.view`.

### 6.1 Scope

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

### 6.2 Why it is a service and not the engine router

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

### 6.3 Configuration model

One document, versioned, applied atomically. These records are the `admin`
interface's types; §6.4 shows the functions.

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

### 6.4 WIT outline

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
§6.3; they are part of this interface.

### 6.5 Routes, CLI and UI

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

### 6.6 Security rules

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

### 6.7 Privileged ports

Default binds are `:8080` and `:8443` so a developer can run the stack without
root. Production uses one of:

- systemd socket activation, with the service inheriting the listening sockets;
- `CAP_NET_BIND_SERVICE` on the service binary;
- an operator-managed port forward.

The service reports a bind failure as a typed error and keeps the previous
listener; it does not retry in a tight loop.

### 6.8 Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| P1 | Config document, HTTP listener, host/path routing, upstreams, status | §3.2 |
| P2 | TLS from `{shared}/tls`, SNI, redirect, certificate reload | P1, §4 C2 |
| P3 | WebSocket, HTTP/2, header policy, body limits | P2 |
| P4 | Access log stream, counters, test upstream, atomic apply/rollback | P3, §3.4 |
| P5 | Privileged-port deployment, sandbox caps, hardening | P4, §3.5 |

### 6.9 Risks

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

### 6.10 Acceptance

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

## 7. Container Manager

Bundle `org.anthracite.containers`, package `anthracite:containers@1.0.0`,
service class `admin`, one `panel.view`.

### 7.1 Scope

- Inventory containers, images, volumes and networks from a Docker-compatible
  runtime API (Docker Engine or Podman).
- Container details: image, command, state, health, ports, labels, created,
  restart policy, mounts (named volumes only), network.
- Lifecycle: start, stop with timeout, restart, pause/unpause, kill with an
  allowlisted signal, remove with a dependent check and confirmation.
- Create from an image: name, command, environment, published ports, named
  volumes, network, restart policy, labels. No bind mounts, no privileged mode,
  no host namespaces, no devices.
- Pull an image with progress; list and remove images with a dependent check.
- Logs: tail and follow, ANSI preserved, rendered by the terminal layer.
- Exec: a shell or command in the container, over the terminal renderer, with
  resize and close.
- Stats: CPU, memory, network and PIDs, sampled and streamed.

**Non-goals:** Kubernetes, Swarm, Compose stacks, image builds, a registry UI,
vulnerability scanning, bind mounts, privileged containers, host PID/network
namespaces, device passthrough, or rolling updates.

### 7.2 Authority

- Class `admin`; the runtime socket is a literal `--runtime` argv entry (for
  example `/var/run/docker.sock` or `$XDG_RUNTIME_DIR/podman/podman.sock`) shown
  at approval and frozen in the grant. The service connects to it; it never
  shells out to `docker`.
- The runtime socket is **root-equivalent** for containers. The approval text
  says so. A compromised service can control every workload the runtime owns;
  the service sandbox and the audit bridge are the mitigations, not a claim that
  the socket is safe.
- Every mutation is audited with the container/image id and the action, never the
  environment values.
- The service receives only the runtime socket path approved in argv; it does
  not receive filesystem paths through WIT or the engine. Named volumes are
  runtime objects; their host mountpoint is not rendered in the UI.

### 7.3 WIT outline

```wit
package anthracite:containers@1.0.0;

interface runtime {
  use anth:error/kinds@1.0.0.{error};

  record label { name: string, value: string }
  record env-var { name: string, value: string }
  record volume-mount { name: string, path: string }  // path is inside the container

  record container {
    id: string,
    name: string,
    image: string,
    state: string,
    status: string,
    health: option<string>,
    created-at: u64,
    ports: list<string>,
    labels: list<label>,
  }

  record image { id: string, tags: list<string>, size: u64, created-at: u64, digest: option<string> }
  record volume { name: string, driver: string, labels: list<label> }
  record network { name: string, driver: string, subnet: option<string>, containers: u32 }

  record create-request {
    image: string,
    name: option<string>,
    command: list<string>,
    env: list<env-var>,
    ports: list<string>,
    volumes: list<volume-mount>,
    network: option<string>,
    restart: string,
    labels: list<label>,
  }

  record sample { cpu-percent: f64, memory-bytes: u64, memory-limit: u64, net-rx: u64, net-tx: u64, pids: u64 }

  list-containers: func() -> list<container>;
  inspect: func(id: string) -> option<container>;
  create: func(request: create-request) -> result<container, error>;
  start: func(id: string) -> result<bool, error>;
  stop: func(id: string, timeout-s: u32) -> result<bool, error>;
  restart: func(id: string, timeout-s: u32) -> result<bool, error>;
  pause: func(id: string) -> result<bool, error>;
  unpause: func(id: string) -> result<bool, error>;
  kill: func(id: string, signal: string) -> result<bool, error>;
  remove: func(id: string, force: bool) -> result<bool, error>;
  logs: func(id: string, tail: u32, follow: bool) -> result<stream<list<u8>>, error>;
  stats: func(id: string) -> result<stream<sample>, error>;
  exec: func(id: string, command: list<string>, tty: bool, cols: u32, rows: u32) -> result<string, error>;
  exec-attach: func(session: string) -> result<stream<list<u8>>, error>;
  exec-write: func(session: string, data: list<u8>) -> result<bool, error>;
  exec-resize: func(session: string, cols: u32, rows: u32) -> result<bool, error>;
  exec-close: func(session: string) -> result<bool, error>;
  list-images: func() -> list<image>;
  pull: func(reference: string) -> result<stream<list<u8>>, error>;
  remove-image: func(id: string, force: bool) -> result<bool, error>;
  list-volumes: func() -> list<volume>;
  create-volume: func(name: string) -> result<volume, error>;
  remove-volume: func(name: string, force: bool) -> result<bool, error>;
  list-networks: func() -> list<network>;
}
```

Logs are raw bytes so ANSI survives; stdout and stderr are merged unless the
runtime exposes them separately. Exec reuses the terminal's session shape; the
panel's terminal SDK renders it, so the container app does not reinvent the key
bar or the resize logic.

### 7.4 Routes, CLI and UI

| Surface | Shape |
| --- | --- |
| `GET /containers` | `list-containers` |
| `GET /container?id=…` | `inspect` |
| `POST /containers` | `create` |
| `POST /container/start`, `/stop`, `/restart`, `/pause`, `/unpause`, `/kill`, `/remove` | one exact route per action; body carries the id and options |
| `GET /container/logs?id=…&tail=…&follow=…` | `stream = true`, `application/octet-stream` |
| `GET /container/stats?id=…` | `stream = true`, `events = true` |
| `POST /container/exec`, `/exec-write`, `/exec-resize`, `/exec-close` | one exact route per call |
| `GET /container/exec/attach?session=…` | `exec-attach`, `stream = true`, `application/octet-stream` |
| `GET /images`, `POST /images/pull`, `POST /images/remove` | list, pull stream, remove |
| `GET /volumes`, `POST /volumes/create`, `POST /volumes/remove` | list, create, remove |
| `GET /networks` | list |
| CLI `containers list`, `start`, `stop`, `logs`, `images` | the same calls |

The view is a container table with state and health, a details drawer, a log
pane, a stats strip and an exec tab. The FAB "Create container" opens a modal;
"Refresh" is the second action. Destructive actions use a confirmation modal
inside the app, not a panel modal.

### 7.5 Runtime compatibility

- Docker Engine API v1.43+ over a Unix socket; Podman's compatible API is the
  second target, including rootless. The adapter is internal: the service speaks
  the HTTP API, not the CLI.
- A missing socket, a permission error and an unsupported API version are
  distinct typed errors; the app degrades, the engine does not.
- Registry authentication is the runtime's business. The app does not store
  registry passwords in v0.7.0.

### 7.6 Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| K1 | Runtime client, inventory, details, stats read, logs tail | §3.2 |
| K2 | Lifecycle, remove guards, audit | K1, §3.7 |
| K3 | Create, pull, images, volumes, networks | K2, §3.4 |
| K4 | Logs follow, stats stream, exec over the terminal renderer | K3, §3.4 |
| K5 | Rootless Podman QA, sandbox, hardening | K4, §3.5 |

### 7.7 Risks

- **Root-equivalent runtime socket.** The approval text is explicit; the service
  is system-tier and sandboxed in a release. A third-party service with this
  socket is not a supported deployment.
- **Image supply chain.** The app shows the image reference and digest; it does
  not build or scan. Pulling `latest` is allowed but warned.
- **Log volume.** Follow logs are budgeted; a chatty container cannot exhaust the
  panel.
- **Orphaned resources.** Removing a container leaves its named volumes unless
  the operator asks; the UI lists them.
- **Exec is a shell.** It runs inside the container, with the container's
  privileges. The container must not be privileged; v0.7.0 never creates one.

### 7.8 Acceptance

- The inventory works against Docker and rootless Podman.
- Start/stop/restart/remove work; a remove with dependent volumes asks first.
- Create rejects a bind mount, a privileged flag and a host namespace.
- Logs follow with ANSI and stop at the budget; stats stream at the configured
  interval.
- Exec opens, resizes, writes and closes through the terminal renderer.
- A missing runtime socket shows a clear degraded state and does not affect the
  other apps.

---

## 8. App Store

Bundle `org.anthracite.store`, package `anthracite:store@1.0.0`, service class
`socket`, one `panel.view`.

### 8.1 Scope

- Aggregate one or more **sources**: a signed static index, an OCI artifact
  repository, a GitHub releases index, or a local directory for air-gapped
  installs.
- Search and browse by name, publisher, category, interface and tier hint.
- App details: description, license, homepage, changelog, versions, channels,
  declared interfaces, service class and argv (from the signed manifest after
  download), bundle digest and signer fingerprint.
- Download a version as a digest-pinned stream into the cache.
- Compare installed versions with the catalogue and report updates. **No silent
  update, no auto-install, no auto-approval.**
- Cache indexes and bundles, verify every read, and serve offline from the cache.
- Show yanked versions and index revocations as warnings; the engine's trust is
  still authoritative.

**Non-goals:** payments, licensing enforcement, ratings/reviews, telemetry,
auto-update, a hosted store, or an app-visible operator session.

### 8.2 The authority boundary

The store is a **catalogue**, not an installer.

- The store service has `socket` egress to configured index and bundle URLs, and
  a `{data}` cache. It has no operator session, no engine API credential and no
  install verb.
- The panel's manager performs the install with the operator's session:
  1. The manager finds composed apps whose surface exports
     `anthracite:store/catalog@1.0.0` (§3.6).
  2. It reads the catalogue and the selected version through that app's route,
     with the same session and origin checks as any app call.
  3. It downloads the bundle as a stream, checks the advertised SHA-256 and the
     size cap, then `POST`s it to `/api/engine/apps`.
  4. The engine verifies the signature, the manifest and the WIT; it computes
     the grant. The manager shows the computed risk, the service class/argv, the
     declared interfaces and the authority diff, and requires explicit approval.
  5. The store is told nothing about the session or the grant; the engine's
     audit is the record.

This keeps D-Manager-Panel's rule: an app module never holds engine management
authority. The store view browses; the service caches; the install button lives
in the manager, where the session already is.

**Proposed alternative (not v0.7.0):** a generic `panel.intent` vocabulary,
where an app asks the panel to perform an install with a digest and the panel
confirms. It is a contract addition and is deferred until the manager flow
proves too clumsy. The default is the manager-mediated flow above.

### 8.3 WIT outline

```wit
package anthracite:store@1.0.0;

interface catalog {
  use anth:error/kinds@1.0.0.{error};

  enum channel { stable, beta, nightly }

  record source { id: string, name: string, kind: string, url: string, trusted: bool }
  record app-summary { id: string, name: string, summary: string, publisher: string, categories: list<string>, latest: string }
  record version {
    version: string,
    channel: channel,
    published-at: u64,
    sha256: string,
    size: u64,
    bundle-url: string,
    signer: string,
    yanked: bool,
  }
  record app-detail {
    id: string,
    name: string,
    summary: string,
    description: string,
    publisher: string,
    license: string,
    homepage: option<string>,
    categories: list<string>,
    versions: list<version>,
  }
  record installed { id: string, version: string, channel: channel }
  record update { id: string, installed: string, latest: string, channel: channel }

  sources: func() -> list<source>;
  refresh: func(source: string) -> result<source, error>;
  search: func(query: string, categories: option<list<string>>, limit: u32) -> list<app-summary>;
  app: func(id: string) -> option<app-detail>;
  versions: func(id: string) -> list<version>;
  download: func(id: string, version: string) -> result<stream<list<u8>>, error>;
  check-updates: func(installed: list<installed>) -> list<update>;
}
```

`download` is a stream route. The service verifies the cached or fetched bytes
against `sha256` before the first chunk; a mismatch is an error before the
stream starts, not a truncated bundle.

### 8.4 Routes and CLI

| Surface | Shape |
| --- | --- |
| `GET /sources` | `sources` |
| `POST /sources/refresh` | `refresh` (body carries the source id) |
| `GET /search?query=…&limit=…` | `search` |
| `GET /app?id=…` | `app` |
| `GET /versions?id=…` | `versions` |
| `GET /download?id=…&version=…` | `download`, `stream = true`, `application/octet-stream` |
| `POST /updates` | `check-updates` (body carries the installed list) |
| CLI `store sources`, `search`, `show`, `updates` | the same calls |

The CLI is read-only for installs by design; there is no `store install` verb.

### 8.5 Index format and trust

A source serves two files:

```
index.json        the catalogue, exact bytes
index.json.sig    detached Ed25519 signature over those exact bytes
```

- The signature is over the **bytes as served**, not a reserialized JSON. This
  avoids a canonical-JSON argument and matches the bundle rule: verify what you
  received.
- The public key is configured in the store service and shown at approval. An
  index cannot introduce its own trust root.
- The index is advisory metadata. The bundle's signature, manifest hash, WIT and
  grant are verified by the engine; the store's "publisher" label is not a trust
  decision.
- The index may list yanked versions and revoked signers. The UI warns; the
  engine refuses what its keyring or grant refuses.

Minimal index:

```json
{
  "schema": 1,
  "store": { "id": "org.example.store", "name": "Example Store" },
  "apps": [
    {
      "id": "org.example.notes",
      "name": "Notes",
      "summary": "A tiny notes app.",
      "publisher": "Example",
      "categories": ["productivity"],
      "versions": [
        {
          "version": "1.2.0",
          "channel": "stable",
          "published_at": 1760000000,
          "sha256": "…",
          "size": 48213,
          "bundle_url": "https://store.example/notes-1.2.0.anth",
          "signer": "…",
          "yanked": false
        }
      ]
    }
  ]
}
```

### 8.6 Provider-agnostic sources

- `native`: the signed index above over HTTPS.
- `oci`: an OCI repository whose artifact manifest points at the index and
  bundle blobs; anonymous pull in v1.
- `github`: release assets, with the index and signature as assets.
- `local`: a directory under `{shared}/store` for offline/air-gapped installs;
  the root is `--local-root {shared}/store` in argv.

Vendor-specific stores are sources, not separate provider bundles: the
`anthracite:` namespace is reserved for the system tier, so a third-party app
cannot export `anthracite:store/catalog@1.0.0`. The official store aggregates
sources. A future optional-interface negotiation could let a third-party provider
register, but it is not v0.7.0 (§10, P-Store-Provider).

### 8.7 UI

The view is a search box, category filters and a card grid. A details page shows
versions, changelog, license, declared interfaces and the source's trust state.
The view is browse-only: the install button lives in the manager's Store tab,
which reads the same provider and owns the session. The FAB "Refresh catalogues"
is icon-only; a second action opens the sources modal.

Icons come from the bundle after download, not from remote image URLs, so the
panel does not leak the operator's address to a catalogue. Before download, the
card uses initials.

### 8.8 Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| S1 | Index format, signature verification, native source, cache, search | §3.2, §3.3, §3.4 |
| S2 | View, details, source management, refresh | S1 |
| S3 | Manager store tab, download handoff, install/approval flow | S2, §3.6 |
| S4 | Updates, channels, yanked/revocation warnings | S3 |
| S5 | OCI, GitHub and local sources, offline mode, hardening | S4, §3.5 |

### 8.9 Risks

- **Malicious catalogue.** The index is signed, bundles are digest-pinned and the
  engine verifies signatures. The store never approves.
- **SSRF and privacy.** Source URLs are operator-configured. The service refuses
  loopback, link-local and metadata addresses unless explicitly allowed, and
  remote icons are not fetched by the panel.
- **Cache poisoning.** Cache keys include the digest; a read re-hashes. A
  corrupted cache is evicted, not served.
- **Namespace squatting.** The engine's id and signer rules decide; the store
  displays, it does not arbitrate.
- **Stale metadata.** ETag/If-None-Match and a visible `refreshed_at`; offline
  mode labels the age.

### 8.10 Acceptance

- A signed index with one app searches, shows details and downloads a bundle
  whose digest matches.
- A tampered index signature is refused and the previous cache is kept.
- A tampered bundle is refused before the manager can install it.
- The manager installs the bundle, shows the engine-computed risk and requires
  approval; the store never sees the session.
- An update is reported, not applied; applying it re-runs install and approval.
- A local source works with no network.

---

## 9. Composition, sequencing and removal

### 9.1 The edge composition

`compositions/edge.toml` composes the system apps plus the five business apps.
It is a normal composition: no engine mode, no feature flag.

```toml
[[profile]]
id = "edge"
apps = [
  "org.anthracite.fs",
  "org.anthracite.terminal",
  "org.anthracite.proc",
  "org.anthracite.net",
  "org.anthracite.storage",
  "org.anthracite.accounts",
  "org.anthracite.firewall",
  "org.anthracite.certs",
  "org.anthracite.acme",
  "org.anthracite.proxy",
  "org.anthracite.containers",
  "org.anthracite.store",
]
```

The management app (`manager`) is panel chrome, not a bundle
(D-Manager-Panel); it is the one place that holds the operator session for
installs and approvals.

### 9.2 Order

1. **Phase 0 — prerequisites** (§3).
2. **Certificates** (§4): inventory first, then import and publish.
3. **Proxy** (§6): HTTP first, then TLS from the published directory.
4. **ACME** (§5): HTTP-01 once the proxy serves the token directory, then
   renewal, then DNS-01.
5. **Containers** (§7): independent; can run in parallel with 3–4.
6. **Store** (§8): independent; the manager handoff needs §3.6.
7. **Hardening and release** (§9.5).

Each phase is a PR-sized slice with its own acceptance, tests and docs. A phase
that cannot be demonstrated on a loopback host (ACME staging, rootless Podman)
ships with a fixture and a manual QA row, not a claim.

### 9.3 Removal behavior

The M4 rule holds: removing an app leaves a working system with that section
gone.

| Removed | Expected behavior |
| --- | --- |
| `certs` | Proxy keeps already-published files until they expire; ACME cannot store new material and reports `issued-unstored`; the panel shows the missing provider |
| `proxy` | HTTP-01 fails the self-check; DNS-01 still works; existing certificates remain; containers and store unaffected |
| `acme` | Imported and system certificates still work; no renewals; existing orders stay in the app's `{data}` if it is reinstalled |
| `containers` | The workload section disappears; no other app degrades |
| `store` | The manager's store tab disappears; installs from the engine API still work |
| all five | The system composition boots and behaves as v0.6.0 |

The golden test asserts zero `fatal` and the exact expected `degraded` set for
each removal. A missing provider is a degraded consumer, never a boot failure.

### 9.4 Data and uninstall

Uninstalling an app does not delete its `{data}` or the files it published under
`{shared}`. The panel warns before removing a certificate or a proxy config; an
operator cleanup is explicit. This is the existing state rule, restated because
these apps hold secrets.

### 9.5 Release gate

- Service sandbox and system tier are in the release build.
- The conformance kit passes for all five bundles.
- The removal matrix above is automated.
- Manual QA covers: Let's Encrypt staging issue/renew via HTTP-01 and DNS-01,
  TLS reload, WebSocket through the proxy, Docker and rootless Podman, install
  from a local and a remote source, and a tampered bundle.
- No secret appears in a route, log, audit record, crash dump or the panel.
- Docs: the app pages, the WIT reference, the index format, the approval text
  for `admin`/`host` services and the port/capability guide.

---

## 10. Proposed decisions

These are the open points the implementation must settle. They are proposed here
and repeated in [§15](15-open-decisions.md) as D9–D20; none is binding until it
enters [§16](16-decision-log.md).

| ID | Decision | Recommendation | Why |
| --- | --- | --- | --- |
| P-Service-Broker | How a service calls another service | Private `{broker}` socket and generated clients, authorized by the grant | A file drop or panel courier puts keys or authority in the wrong process |
| P-Listener | How a service declares listeners and egress | Manifest v4 optional `listen`/`egress`, shown at approval and frozen; v3 stays accepted | `argv` is a convention; approval needs a contract |
| P-Secrets | Where service secrets live | `{data}` `0600`, write-only from the panel, never in WIT output or logs | `anth:store/kv` is plaintext and readable by design |
| P-Proxy-Config | Partial edits vs whole document | Whole-document `apply` with a version and last-good rollback | A half-applied host list is an outage |
| P-Proxy-Ports | Default ports | `:8080`/`:8443`; production via socket activation or `CAP_NET_BIND_SERVICE` | A developer should not need root to run the stack |
| P-ACME-DNS | How DNS-01 providers plug in | `manual`, `rfc2136` and `webhook` inside the ACME service; vendor adapters additive | The reserved namespace blocks third-party provider bundles today |
| P-Container-Runtime | Which runtimes | Docker-compatible API only (Docker and Podman); no pluggable runtime interface in v0.7.0 | One interface, two runtimes, no false abstraction |
| P-Store-Install | Who may install | The panel manager with the operator session; the store only catalogues and downloads | An app must not hold engine management authority |
| P-Store-Provider | How many store providers | One official aggregator per composition; sources are adapters; optional-interface negotiation is post-v0.7.0 | The composition binds one provider per interface id |
| P-Store-Index | Index trust | Detached Ed25519 over the exact served bytes; bundle digest plus engine signature | Two independent checks, no canonical-JSON argument |
| P-System-Gate | What a release requires | Distribution key, system tier and service sandbox; otherwise a debug preview | `class` is risk today, not a sandbox |
| P-Audit-Bridge | How service actions are audited | `anth:audit/record@1.0.0` through the broker, sanitized detail | Certificate and container actions belong in the engine record |

---

## 11. Global non-goals

- A Kubernetes distribution, an orchestrator or a service mesh.
- A CDN, WAF, cache or L4 load balancer.
- A full PKI, internal CA, HSM or key escrow.
- A hosted store, payments, licensing enforcement or reviews.
- Auto-updates or silent re-approval.
- Multi-tenant isolation inside one composition.
- Windows or macOS host services in v0.7.0. The platform targets Linux; the
  panel remains a browser client.
- Giving a wasm app direct network, key or runtime-socket access. The answer is
  always a service with a declared class, or a typed interface the service
  exports.

---

## 12. Definition of done

v0.7.0 is done when a new host with the edge composition can:

1. import or issue a certificate, publish it, and serve HTTPS through the proxy;
2. renew it without an operator action and show the timeline;
3. route two hosts, one to a container and one to a system app, with WebSocket
   and HTTP/2;
4. list, start, stop, log and exec a container from the panel without SSH;
5. browse a signed catalogue, install a bundle through the manager, see the
   engine-computed risk, and approve it explicitly;
6. remove any of the five apps and keep a working system with the expected
   degradation, asserted by the golden composition;
7. pass the conformance kit, the removal matrix and the manual QA matrix with no
   secret in a log, route, audit record or the panel.

That is the next version: not more apps for their own sake, but the proof that
the authority model survives contact with the things an operator actually runs.
