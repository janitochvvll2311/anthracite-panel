# 21. The next version: business apps

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**, the
> milestone after [v0.6.0](18-v0.6-plan.md).

Status: **accepted**. The scope is binding; the phased plan is
[§29](29-v0.7-plan.md) and the decisions are in [§16](16-decision-log.md)
(D9–D20). Nothing here starts before the start conditions in §2 are met.

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
approval, secret custody — is generic and listed in [§22](22-platform-prerequisites.md); it is not feature logic
in the engine.

---

## Documents

| § | Document | What it settles |
| --- | --- | --- |
| 22 | [Platform prerequisites](22-platform-prerequisites.md) | Broker, listener/egress, secrets, streams, system tier, surface exports, audit and time |
| 23 | [Certificate manager](23-certificate-manager.md) | Inventory, import, custody and publication of TLS material |
| 24 | [ACME client](24-acme-client.md) | Accounts, orders, HTTP-01, DNS-01 and renewal |
| 25 | [Reverse proxy](25-reverse-proxy.md) | Hosts, routes, TLS, WebSockets and atomic apply |
| 26 | [Container manager](26-container-manager.md) | Docker/Podman inventory, lifecycle, logs, stats and exec |
| 27 | [App store](27-app-store.md) | Signed catalogues, sources and the manager-mediated install handoff |

---

## 1. Why these five

The system apps answer "what is on this host?". These answer "what does this host
serve, and to whom?".

| App | Bundle id | The pain it removes | The business value |
| --- | --- | --- | --- |
| [Certificate Manager](23-certificate-manager.md) | `org.anthracite.certs` | Certificates and keys scattered across `/etc`, expiry found by outage | One inventory, expiry and chain status, audited import, keys never rendered |
| [ACME client](24-acme-client.md) | `org.anthracite.acme` | Manual renewals, one vendor's client, wildcard certificates out of reach | Zero-touch issuance and renewal against any RFC 8555 CA, HTTP-01 or DNS-01 |
| [Reverse Proxy](25-reverse-proxy.md) | `org.anthracite.proxy` | Hand-edited nginx, TLS and routing in a text file nobody reviews | Typed host/path routing, TLS termination, WebSockets, atomic apply and logs |
| [Container Manager](26-container-manager.md) | `org.anthracite.containers` | SSH, `docker` on the command line, logs copied by hand | Workload inventory, lifecycle, logs, stats and exec behind the same approval |
| [App Store](27-app-store.md) | `org.anthracite.store` | Copying zips, trusting a publisher by habit, silent updates | Signed catalogues, digest-pinned bundles, explicit install and re-approval |

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
  reach and a `panel.view` module under `web/`; WIT stays the public API ([§19 §4](19-v0.6-baseline.md#4-service-lifecycle-and-wit-rpc)).
- **They have a management plane and a data plane.** The management plane is
  WIT over `POST /__anth/call`, wrapped by routes, CLI and the panel. The data
  plane is native: the proxy serves HTTP, the container manager talks to the
  runtime and bridges logs, the ACME client speaks TLS to the CA. The engine
  never proxies customer traffic.
- **They hold secrets.** Account keys, EAB secrets and DNS tokens live in the
  service's `{data}` directory (`0600`). Private keys live in the certificate
  manager's `{data}` and are published to `{shared}/tls`; every service granted
  `{shared}` can read that directory, so the grant is the boundary, not a
  per-file ACL. None of it is `anth:store/kv` state, and none of it is read back
  through a route, a log line or the panel. A manual certificate import is the
  one explicit write path, and the UI calls it out.
- **They stream.** Order progress, proxy access logs, container logs and stats,
  and bundle downloads are `stream<T>` under the rules of
  [§20](20-streams.md). A poll-only version is a fallback, not the design.
- **They are system-tier in a release.** Their class is `socket`, `admin` or
  `host`; they need the distribution key and the service sandbox before an
  operator can call the deployment supported ([§22 §5](22-platform-prerequisites.md#5-system-tier-and-service-sandbox)).

---

## 2. Start conditions

v0.7.0 starts when v0.6.0 is tagged — it is, at `29e0ef6` — and the work is cut
from a `v0.7.0` branch. Phase 0 then implements the platform prerequisites; the
app phases follow ([§29](29-v0.7-plan.md)).

### 2.1 v0.6.0 is complete

- M4 closes with the golden system composition and the proof that removing `net`
  leaves a working system ([§18](18-v0.6-plan.md)).
- Typed events and stream quotas are implemented ([§20](20-streams.md)).
- The conformance kit, `fetch-wit` and the audit/diff UI are shipped, and the
  sandbox baseline (L0) is in place. The layers a supported release needs — L1
  plus the port capability — are phase 0b, not a v0.6.0 deliverable
  ([§28](28-service-sandbox.md)).
- A distribution key can sign the system tier, and `scripts/release.sh` proves the
  binaries and the bundles agree. Producing the first real release key is an
  operator step, not a code gap.

### 2.2 The platform prerequisites are phase 0

They are part of v0.7.0, not a gate before it. Phase 0 implements
[§22](22-platform-prerequisites.md) and the five app phases start after. Without
them, one of the five apps either cannot be written without ambient authority or
cannot be shipped honestly.

### 2.3 The scope is frozen

The five apps are the milestone. A sixth app, a hosted service, a payment flow or
a Kubernetes integration is a different document and a different approval.

---

## 3. The edge composition

`compositions/edge.toml` composes the system apps plus the five business apps.
It is a normal composition: no engine mode, no feature flag.

```toml
version = 1

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

## 4. Order

The phases and their PR definitions are [§29](29-v0.7-plan.md). In order:

1. **0a — manifest v4** ([§22](22-platform-prerequisites.md)): roots, listeners,
   egress and `{broker}`.
2. **0b — sandbox L1 and the port capability** ([§28](28-service-sandbox.md)).
3. **0c — broker, audit bridge and surface exports** ([§22](22-platform-prerequisites.md)).
4. **Certificates** ([§23](23-certificate-manager.md)): inventory first, then
   import and publish.
5. **Proxy** ([§25](25-reverse-proxy.md)): HTTP first, then TLS from the published
   directory, then WebSocket and HTTP/2.
6. **ACME** ([§24](24-acme-client.md)): HTTP-01 once the proxy serves the token
   directory, then renewal, then DNS-01.
7. **Containers** ([§26](26-container-manager.md)): independent of 4–6.
8. **Store** ([§27](27-app-store.md)): independent; the manager handoff needs the
   surface exports of 0c.
9. **Hardening and release** (§7 below).

Each phase is a PR-sized slice with its own acceptance, tests and docs. A phase
that cannot be demonstrated on a loopback host (ACME production, Docker,
rootless Podman) ships with a fixture (Pebble, a fake runtime) and a manual QA
row, not a claim.

## 5. Removal behavior

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
Removing `certs` while `acme` stays composed needs
`[composition] allow-degraded = true`; without it the unresolved import is the
strict fatal error of [§8.5](08-composition.md#85-missing-and-failed), because
editing the composition is what fixes it.

The matrix is `engine/api/tests/golden.rs`; the edge composition is
`compositions/edge.toml`.

## 6. Data and uninstall

Uninstalling an app does not delete its `{data}` or the files it published under
`{shared}`. The panel warns before removing a certificate or a proxy config; an
operator cleanup is explicit. This is the existing state rule, restated because
these apps hold secrets.

## 7. Release gate

- The distribution key, system tier, sandbox L1 and the port capability are in
  the release build. There is no debug-preview release ([§29](29-v0.7-plan.md),
  D19).
- The conformance kit passes for all five bundles.
- The removal matrix above is automated; ACME is exercised against Pebble.
- Manual QA covers: Let's Encrypt staging issue/renew via HTTP-01 and DNS-01,
  TLS reload, WebSocket through the proxy, Docker and rootless Podman, install
  from a local and a remote source, and a tampered bundle.
- No secret appears in a route, log, audit record, crash dump or the panel.
- Docs: the app pages, the WIT reference, the index format, the approval text
  for `admin`/`host` services and the port/capability guide.

---

## 8. Decisions

The recommendations below are **accepted and binding** as D9–D20 in
[§16](16-decision-log.md); [§15](15-open-decisions.md) records them as decided.
The review amended three of them: the broker has no file-drop fallback in
v0.7.0, there is no debug-preview release, and privileged ports use
`CAP_NET_BIND_SERVICE` rather than socket activation.

| ID | Decision | Recommendation | Why |
| --- | --- | --- | --- |
| P-Service-Broker | How a service calls another service | Private `{broker}` socket and generated clients, authorized by the grant; no file-drop fallback in v0.7.0 | A file drop or panel courier puts keys or authority in the wrong process |
| P-Listener | How a service declares roots, listeners and egress | Manifest v4 optional `roots`/`listen`/`egress`, shown at approval and frozen; v3 stays accepted; roots and ports are enforced in v0.7.0, egress is intent until network isolation |
| P-Secrets | Where service secrets live | `{data}` `0600`, write-only from the panel, never in WIT output or logs | `anth:store/kv` is plaintext and readable by design |
| P-Proxy-Config | Partial edits vs whole document | Whole-document `apply` with a version and last-good rollback | A half-applied host list is an outage |
| P-Proxy-Ports | Default ports | `:8080`/`:8443`; production `:80`/`:443` via `CAP_NET_BIND_SERVICE` granted by the supervisor; socket activation is out of scope | A developer should not need root to run the stack, and the release should not depend on a host init system |
| P-ACME-DNS | How DNS-01 providers plug in | `manual`, `rfc2136` and `webhook` inside the ACME service; vendor adapters additive | A separate provider bundle needs a contract and a trust decision |
| P-Container-Runtime | Which runtimes | Docker-compatible API only (Docker and Podman); no pluggable runtime interface in v0.7.0 | One interface, two runtimes, no false abstraction |
| P-Store-Install | Who may install | The panel manager with the operator session, including an explicit `key add` when the publisher is untrusted; the store only catalogues and downloads | An app must not hold engine management authority |
| P-Store-Provider | How many store providers | One official aggregator per composition; sources are adapters; optional-interface negotiation is post-v0.7.0 | The composition binds one provider per interface id, and tier ownership is not enforced yet |
| P-Store-Index | Index trust | Detached Ed25519 over the exact served bytes, produced by `anth-sign file`; bundle digest plus engine signature stay independent | Two independent checks, no canonical-JSON argument |
| P-System-Gate | What a release requires | Distribution key, system tier, sandbox L1 and the port capability; a debug preview is not a release | `class` is risk today, not a sandbox; shipping unsandboxed host reach under a release name would be dishonest |
| P-Audit-Bridge | How service actions are audited | `anth:audit/record@1.0.0` through the broker, sanitized detail | Certificate and container actions belong in the engine record |

---

## 9. Global non-goals

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

## 10. Definition of done

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
