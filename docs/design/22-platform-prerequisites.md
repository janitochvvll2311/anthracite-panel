# 22. Platform prerequisites

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**.
> Start conditions, sequencing and the app list: [§21](21-next-version-apps.md).

These land in v0.7.0 phase 0 unless the table says otherwise. Each is generic
mechanism: none mentions a certificate, a container or a store.

## 1. Service call broker

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

## 2. Listener and egress intent in the manifest

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
- the service runs with a privilege drop, a namespace or Landlock/seccomp
  profile, and the port/capability set it declared.

Until then v0.7.0 is a debug preview. That is an honest release note, not a
limitation to bury.

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
engine stamps `actor`, `app` and time. If this slips, each service writes a local
JSONL audit in `{data}/audit.jsonl` and the panel shows it; the release notes say
the records are app-local, not engine-global.

## 8. Time and scheduling

There is no engine cron. A service owns its timers:

- ACME renewal and store refresh run as a loop inside the service, with jitter,
  bounded backoff and persisted state, so a restart resumes rather than restarts
  the schedule.
- Wall-clock correctness matters for certificate expiry and ACME. The service
  reports clock skew in `status`; a large skew is a warning, not a reason to
  silently retry faster.

---
