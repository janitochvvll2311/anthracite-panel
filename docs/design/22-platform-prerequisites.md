# 22. Platform prerequisites

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**.
> Start conditions, sequencing and the app list: [§21](21-next-version-apps.md).

These land in v0.7.0 phase 0 unless the table says otherwise. Each is generic
mechanism: none mentions a certificate, a container or a store.

## 1. Service call broker

A service must be able to call another service's interface through the grant,
not through the public router.

- **Implemented in v0.7.0 (phase 0c):** the engine binds
  `state/broker/<app-id>.sock` for every composed app and hands the path over as
  `{broker}`. The socket is `0600` and, when the engine runs as root, owned by
  the service uid (the directory stays `0711`); the broker then checks
  `SO_PEERCRED` before reading a request, so another service cannot open a
  peer's socket and inherit its grant. Without root the services share the
  engine uid and that check cannot tell them apart (the uid-layer gap in
  [Services](../../guide/services.md#supervision)). The service posts
  `interface#function` plus `input` to `/__anth/call`; the broker checks that
  the caller's grant imports the interface, resolves the composed provider,
  reuses `AppCaller` (and therefore the provider's budget), and audits every call
  as `broker.call`. A call to an interface the caller does not import is a
  denial with the caller in the record. `POST /__anth/stream` carries the same
  authority for a streamed function; the provider's manifest budget bounds the
  chunks and the stream ends with `exhausted` when it runs out.
- **Audit bridge:** `anth:audit/record@1.0.0` is served on the same socket; the
  engine sanitizes the app-supplied detail before writing it.
- **Client:** `anth_service::Broker` speaks the protocol for a service author;
  `SurfaceApp.exports` lists every app's interfaces so a panel can find a
  provider without reading a manifest.
- **Why it gates the plan:** ACME imports the certificate store; a future
  DNS-01 provider bundle would be the second typed consumer. A file drop or a
  panel courier would put private keys or operator authority in the wrong
  process.
- **No fallback.** The file-drop contingency is not in v0.7.0: if the broker
  slips, phase 0c and every phase that needs it slips (D9,
  [§29](29-v0.7-plan.md)).

## 2. Listener and egress intent in the manifest

The operator must see what a service intends to bind and where it intends to
connect before approving it. `argv` shows flags, but a flag is a convention, not
a contract.

Manifest v4 (phase 0a, D10): optional `[service]` keys, with v3 still accepted.

```toml
[service]
path = "bin/proxy"
sha256 = "…"
class = "socket"
argv = ["--socket", "{socket}", "--data", "{data}", "--http", ":8080", "--https", ":8443"]
listen = ["tcp:8080", "tcp:8443"]
egress = ["any"]                    # or ["host:443", "10.0.0.0/8:5432"]
```

- `listen` and `egress` are **shown at approval and frozen in the grant**.
  `roots` and the privileged listeners are enforced by phase 0b (L1 and the port
  capability); `egress` stays intent, because network isolation is L2 and comes
  after v0.7.0. The approval text says which is which.
- The engine refuses a declared listener that is not a `tcp:` or `udp:` address.
  It does not allocate ports, and it does not reserve them.
- In a release, the sandbox maps `listen` to `CAP_NET_BIND_SERVICE` and `roots`
  to the filesystem policy. A debug build still gets L1 and the capability: the
  sandbox is not a release-only feature.
- A service that declares neither is unchanged (v3 semantics).

## 3. Secret custody

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

## 4. Streams, quotas and backpressure

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

## 5. System tier and service sandbox

`class` is risk today; the child still runs as the engine user
([§19 §4](19-v0.6-baseline.md#4-service-lifecycle-and-wit-rpc)). A proxy that
binds `:443`, a certificate manager that reads `/etc/ssl` and a container manager
that holds the runtime socket are only supportable when:

- the bundle is signed by the distribution key (`system` tier), and
- the service runs with a privilege drop, a filesystem policy and the
  port/capability set it declared.

Phase 0b of [§29](29-v0.7-plan.md) delivers that: per-service uid, Landlock
rules from `roots`, and `CAP_NET_BIND_SERVICE` for a declared privileged
listener. A host that cannot provide them refuses the launch; v0.7.0 has no
debug-preview release (D19).

## 6. Surface exports

The panel manager must discover providers without hard-coding an app id. Add the
composed app's exported interfaces to `SurfaceApp` in
`GET /api/engine/composition`:

```json
{ "id": "org.anthracite.store", "exports": ["anthracite:store/catalog@1.0.0"], "…": "…" }
```

This is generic surface data. The manager can then find the store provider, the
certificate provider or any future provider by interface, not by name. Before
using a provider it checks that app's descriptor tier; the store and certificate
roles are system-tier, and a third-party app claiming the interface is shown as
unsupported. The composition still binds one provider per interface id.

## 7. A service audit bridge

Container start/stop/exec, certificate import and proxy apply are operator
actions that must appear in the engine audit (D-Audit), next to installs and
approvals.

Proposed: an `anth:audit/record@1.0.0` host call available to services through
the broker, write-only, with `action`, `result` and a sanitized `detail`. The
engine stamps `actor`, `app` and time. The broker serves it as a native
interface; there is no app-local audit fallback in v0.7.0 (D20).

## 8. Time and scheduling

There is no engine cron. A service owns its timers:

- ACME renewal and store refresh run as a loop inside the service, with jitter,
  bounded backoff and persisted state, so a restart resumes rather than restarts
  the schedule.
- Wall-clock correctness matters for certificate expiry and ACME. The service
  reports clock skew in `status`; a large skew is a warning, not a reason to
  silently retry faster.

---
