# 6. The engine

> Part of the [design index](../../DESIGN.md).

### 6.1 Subsystems

| Subsystem | Contract |
| --- | --- |
| **boot** | Read config, open the store, load the composition, start apps in dependency order, expose the surface. No feature logic. |
| **router** | One HTTP surface over the Unix socket and TCP: `/health`, `/api/health`, `/api/engine/*`, `/api/apps/<id>/*`, `/api/apps/<id>/assets/*`. No `/`. Auth, origin and CSP live here and only here. |
| **registry** | Install, list, verify, remove bundles. Verifies zip paths, signatures, hashes, manifest shape and WIT. Never grants. |
| **composer** | Resolve `composition.toml` + installed manifests into a surface. Deterministic. Reports unresolved/duplicate/degraded. |
| **supervisor** | Own the process tree and a registry of component instances with `acquire`/`release`/`evict`. Restart policy, timeouts, resource budgets, teardown. Child processes arrive with services in v0.6.0. |
| **runtime** | Behind one `async` trait: instantiate components with wasmtime, enforce the budget, validate the component world against the grant, satisfy host imports through the broker, link app imports. |
| **broker** | The only path from app code to the host. Validates against the grant, enforces quotas, records denials. |
| **store** | Per-app namespaces exposed as interfaces: `anth:store/kv` in v0.5.0, `anth:store/blobs` with transfers in v0.6.0. No paths, no sharing. |
| **audit** | Append-only record of install/approve/compose/enable/disable/denials. |

### 6.2 What the engine must not contain

This list is the guardrail. A pull request that violates it is a design change,
not a feature.

| The engine must not… | Because |
| --- | --- |
| know about files, terminals, processes, network, storage or accounts | those are apps (`org.anth.*`) |
| enumerate feature endpoints in the router | routes come from the surface, computed at compose time |
| **serve any UI, or own a slot** | the engine is headless; a panel hosts apps and declares slots |
| hard-code a CLI verb per feature | CLI verbs are projected from WIT ([§7.9](07-app.md#79-cli-and-clients)) |
| grant authority at install time | approval is a separate, explicit act |
| trust the manifest at runtime | the grant is the contract |
| grow the native capability set casually | the set is closed by design ([§9.2](09-authority.md#92-the-closed-native-capability-set)) |
| hand a filesystem path to an app | a path is ambient authority; state is an interface ([§7.10](07-app.md#710-state)) |
| assume its client runs in the same process | a panel may be remote ([§15](15-open-decisions.md#15-open-decisions)) |

### 6.3 Runtime backend and deployment profiles

v0.5.0 ships **one** backend: **wasmtime with Cranelift**. It is the reference
implementation of the component model, it has fuel accounting, epoch-based
interruption and memory limits, and it is where every document, example and bug
fix lives. It is also the only backend fast enough to keep the "everything heavy
can be an app" rule honest.

What that commits us to, explicitly:

- The daemon must be able to map **executable memory**. Hosts that forbid it
  (`deny_execmem`, grsecurity/PaX, a seccomp policy that rejects `PROT_EXEC`) are
  not supported by this build. This is a documented requirement, not a bug.
- The daemon binary grows by roughly the size of a wasm compiler. Only the daemon
  pays: the CLI in `sdk/rust/cli` is a thin client.

Measured on the spike, x86_64 release without LTO or strip: a host embedding
wasmtime with the component model and Cranelift is **17.1 MB**, and **17.7 MB**
with the async features on. A cold build of `wasmtime` takes about **12 minutes**
on this machine, which is a CI cost, not a product one. A component is **19 KB**
without streams and **53 KB** with them.

**Pulley is deferred, not rejected.** A hardened profile (`anth-engine-pulley`)
that interprets instead of generating code is documented for a later version. We
do not build it, test it or ship it in v0.5.0. What we do keep is the scaffolding
that makes it a build flag rather than a redesign:

- `anth-runtime` is the only crate that names `wasmtime`; nothing above it sees a
  runtime type.
- A single `Backend` enum in `anth-runtime` is the only place a backend is
  chosen.
- Limits are expressed in **fuel and memory**; wall time exists only as a
  backstop, never as the primary budget. A limit means the same thing on both
  backends.
- No precompiled artifact ever enters a grant, so an engine upgrade never
  invalidates an installed app.

The runtime boundary is one `async fn` behind a `Runtime` trait, and its second
implementation is a fake backend used by the engine's own tests. Nothing above
that trait depends on wasmtime's semantics. Together with the instance registry
([§6.1](06-engine.md#61-subsystems)), that is what makes streams a later change *inside* `anth-runtime` instead
of a platform redesign. M0 probes the async toolchain and records the result; it
does not enable it.

Consequence for the app model: **wasm may do heavy compute** — archives, image
decoding, hashing. Service apps exist only for what must touch the kernel: PTYs,
`ip`, `nft`, `sfdisk`.

---

### 6.4 HTTP surface

The API is a set of **resources**, not a list of commands. Actions become resource
mutations wherever a resource exists; `POST` is reserved for the one thing that is
not a resource.

| Resource | GET | PUT | PATCH | DELETE | POST |
| --- | --- | --- | --- | --- | --- |
| `/health` | liveness, unauthenticated | — | — | — | — |
| `/api/health` | readiness, authenticated | — | — | — | — |
| `/api/engine/version` | version and protocol | — | — | — | — |
| `/api/engine/composition` | composition, surface and CLI verbs | replace the document | — | — | — |
| `/api/engine/apps` | catalogue | — | — | — | install, id derived from the bundle |
| `/api/engine/apps/{id}` | descriptor and grant | replace the bundle | `enabled`, alias | uninstall | — |
| `/api/engine/apps/{id}/grant` | frozen grant | approve with the accepted levels | — | revoke | — |
| `/api/engine/audit` | paginated log | — | — | — | — |
| `/api/engine/keys` | trusted keys | — | — | — | add a third-party key |
| `/api/engine/keys/{fingerprint}` | — | — | — | remove | — |
| `/api/engine/operator` | operator identity | — | change password | — | — |
| `/api/engine/session` | current session | — | — | log out | log in |
| `/api/engine/restart` | — | — | — | — | restart |
| `/api/apps/{id}/<path>` | per `[[route]]` | per `[[route]]` | per `[[route]]` | per `[[route]]` | per `[[route]]` |
| `/api/apps/{id}/assets/{*module}` | UI module | — | — | — | — |

Why these verbs:

- **`POST` installs, `PUT` replaces.** The id lives inside the bundle, so a client
  cannot name it before uploading: `POST /apps` derives it and answers `201` with
  `Location`. Replacing an installed app is `PUT /apps/{id}`, where the id is known
  and a bundle declaring a different one is `409`. A retried install cannot
  duplicate: the second attempt finds the id and gets `409 app_exists`.
- **`PUT` approves.** A grant is a document with a known identity; `PUT
  /apps/{id}/grant` freezes exactly the authority the operator accepted.
- **`PATCH` enables, disables and renames.** `{"enabled": false}` is a partial
  mutation of a resource, not an invented action endpoint.
- **`DELETE` removes and revokes.** `DELETE /apps/{id}` uninstalls;
  `DELETE /apps/{id}/grant` revokes the authority and keeps the content.
- **`PUT` replaces the whole composition**, not `POST /compose/add`. The
  composition is a document; adding and removing are client-side edits to it,
  guarded by `ETag` and `If-Match` so a concurrent edit cannot be lost. The
  `ETag` is the strong validator of the canonical TOML (`"<sha256 hex>"`); a
  missing or stale `If-Match` is `412 precondition-failed`, `*` matches any
  existing document, and the check and the write are serialized.
- **`POST` for restart.** It is a process transition, not a resource.

**Protocol.** `GET /api/engine/version` answers
`{"engine":"0.5.0","protocol":1}`. A client that does not know the protocol
refuses to talk instead of guessing: the number changes when a request or response
shape does, not when the engine version does. v0.5.0 is protocol `1` because the
API has nothing in common with the v0.4.0 protocol `4` that preceded it.

Method rules:

- An unsupported method is `405` with an `Allow` header, never a `404`.
- `OPTIONS` is answered by the engine for its own resources, with `Allow`. There
  is no CORS in v0.5.0: the browser always speaks to one origin, whether the panel
  mounted the router or proxied it.
- `HEAD` works on assets, and on app routes that declare it. The engine never
  fabricates a `HEAD` by calling a `GET`.
- `GET`, `HEAD` and `DELETE` carry no body; one is `400`. `PUT`, `PATCH` and
  `POST` have a per-resource cap; an install body is a raw `application/zip`
  capped at 32 MiB by default.
- `PUT` that creates answers `201` with `Location`; that replaces, `200`.
  `PATCH`, `200` with the resource. `DELETE`, `204`, or `404` when absent.
  `If-Match` failure, `412`. Id or provider conflict, `409`.
- `PUT`, `PATCH` and `DELETE` are idempotent; `POST` is not, and no client may
  retry it blindly.

Assets use the hash we already verified:

- `ETag` is the module's `sha256`, and `Cache-Control: immutable`.
- `If-None-Match` answers `304` without reading the file.
- `HEAD` is free.

Health answers two different questions, so it is two endpoints rather than one
endpoint with a mode:

| Endpoint | Auth | Answers | Body |
| --- | --- | --- | --- |
| `GET /health` | no | is the process alive? `200` once the socket is bound | `{"status":"ok"}` |
| `GET /api/health` | yes | can it serve? `200` when ready, `503` while starting or degraded | `{"status":"ready"}` |

Both stay minimal on purpose: no version, no counts, no app names. The detail an
operator needs lives in `GET /api/engine/composition`, which is authenticated and
names what is degraded and why.

`/health` is what systemd and containers probe; `/api/health` is what `anth
doctor` and a panel use before rendering anything.

Operationally: **`/health` is liveness, `/api/health` is readiness.** A `503` from
readiness means "up and needs attention", not "restart me". An orchestrator that
treats readiness as liveness would restart the engine precisely when the operator
needs it to fix the degradation.

App routes declare their verb, and the engine matches it exactly: no implicit
`HEAD`, no automatic `OPTIONS`, no `POST` standing in for `PUT`. `[[route]]`
accepts `GET`, `POST`, `PUT`, `PATCH`, `DELETE` and `HEAD`; `OPTIONS` belongs to
the engine.

A remote panel proxies the API unchanged — method, body and headers. A proxy that
rewrites methods makes the SDK lie.

Errors always use the envelope of [§7.11](07-app.md#711-errors).

**Breaking actions ask for acknowledgment.** `PATCH /apps/{id}` with
`{"enabled":false}`, `DELETE /apps/{id}` and `DELETE /apps/{id}/grant` return `409`
when other composed apps depend on what they remove. The error carries
`detail.dependents` and `detail.acknowledge`; repeating the request with

```
X-Anth-Acknowledge: dependents
```

proceeds, and the engine audits the acknowledgment together with the names of the
apps that were broken. The header is a token list so that future breaking actions
can require their own token without inventing a new mechanism.

This is deliberately a confirmation and not a refusal: in a security contingency
an operator must be able to yank an app immediately, and the record of having done
it knowingly is worth more than a prohibition.

### 6.5 Boot and lifecycle

Boot order, and the reason for it:

1. Config and logging.
2. Store, audit, and first-boot credentials.
3. Registry, grants and `composition.toml`.
4. Resolve the composition into a surface. A static miss is fatal for the apps'
   boot: the daemon reports it and keeps serving so the operator can repair the
   document ([§8.5](08-composition.md#85-missing-and-failed), [§8.7](08-composition.md#87-the-surface-view)); `anth doctor` explains it
   without a daemon.
5. Bind the socket. `/health` answers `200` from here on.
6. Supervisor: instantiate apps in dependency order with a per-app timeout. A
   failed instantiation degrades that app and its dependents.
7. `/api/health` flips to `200 ready`.

Binding before starting apps is deliberate: a slow app must be diagnosable, not
indistinguishable from a hang. Until step 7, app routes answer `503 unavailable`.

**First-boot credentials.** The engine generates an operator name and password,
writes them to `state/credentials.txt` with mode `0600`, and logs only the path.
There is no default account and no secret in the log. The first session carries
`must_change_password`, and every other endpoint answers `403
password_change_required` until the password changes — one gate, not a warning.
Losing the file means `anth-engine --reset-credentials`, which requires
filesystem access to the state directory.

**One operator, for now.** v0.5.0 has a single operator, created on first boot.
`approved_by` in a grant and `actor` in the audit record its name. Multiple
operators, and what each may do, is an open decision ([§15](15-open-decisions.md)).

**The socket authenticates who connects.** The kernel reports the peer's uid and
the engine accepts the service user and root; a TCP connection has no peer
credential, so it serves only `/health` until sessions exist. An administration
group arrives with multi-operator, because it asks the same question — who is
allowed to act.

**Login attempts are limited.** Five failures per minute per source, then
backoff, and every failure is audited. A panel exposed without a rate limit turns
a password into a guessing game.

**Security headers cover the panel too.** The router applies origin checks, CSP
and the rest to every response it produces, including the panel's `/`. The panel
does not get to serve HTML without the engine's headers.

**Mounting a panel.** The engine owns `/api/*`; a panel owns `/` and its own
assets. A panel that registers a colliding route fails to start: the last writer
never wins. The panel reads the composition through the API and never touches the
registry or the grants.

A request that reaches the router in-process has no peer credential, so the
embedding host marks it `Trusted` once it has authenticated the browser; a socket
request carries its credential instead. Sessions (M1) move that decision into the
router and make the marker unnecessary.

**Failure behaviour.**

| Failure | Behaviour |
| --- | --- |
| Invalid config | exit non-zero, naming the file and line |
| Unresolved import (static) | exit non-zero; `anth doctor` explains it offline |
| App traps (dynamic) | engine stays up, app degraded, consumers answer `503` |
| Remote panel cannot reach the engine | panel starts, renders a disconnected state, retries with backoff |

**Restart.** `POST /api/engine/restart` shuts down gracefully and re-execs the
process that hosts the engine, with the same arguments. In a linked panel that
restarts the panel too, which is what "restart the service" means. It answers `202
Accepted` with `Location: /api/health` and never claims success before it is
ready.

### 6.6 The contract crate

`engine/contract` holds the shared vocabulary and the pure rules: identifiers,
manifest, composition, grant, risk, world checks, API DTOs and the error mapping.
No I/O, no `wasmtime`, no async, no HTTP types. If a rule can be a function of
values it lives here; if it needs a file, a socket or a component, it lives
somewhere else.

| Module | Holds |
| --- | --- |
| `id` | `InterfaceId` and `AppId`, parse, validate and render canonically |
| `manifest` | the v2 index: app, wasm, export, import, route, slot, cli, limits |
| `composition` | profiles and providers (`App` or `Engine`) |
| `grant` | granted interfaces, risk, approvals, diff |
| `catalog` | the closed native set and the risk of each interface |
| `world` | the expected world and the comparison against a grant |
| `view` | the API DTOs the SDK re-exports |
| `signature` | the canonical signed message over the manifest set |
| `error` | `ErrorKind` and the single HTTP mapping |

Four decisions inside it:

1. **The API DTOs live here** and the SDK re-exports them as stable. Duplicating
   them in the SDK would create two truths and a translation layer to drift.
2. **Parsing is a feature.** TOML and WIT parsing sit behind `features =
   ["parse"]`, enabled by the engine and the CLI; the clients only deserialize
   JSON and do not carry `wit-parser`.
3. **The catalog lives here**, so `anth doctor` can validate a manifest with no
   daemon running — which is the only moment it matters.
4. **The world check is split.** Contract owns `World` and `check_world`; the
   runtime extracts the real world from wasmtime. The rule is testable with
   values; the runtime supplies only what cannot be faked.

`deny_unknown_fields` is on for every document type: a typo in `[[route]]` must
fail loudly. Documents are TOML and the API is JSON, over the same structs.

### 6.7 Configuration

One TOML file, created on first boot if missing, validated by `anth doctor` with
no daemon. Unknown keys are errors, like every other document.

| Key | Type | Release default | Notes |
| --- | --- | --- | --- |
| `version` | int | `1` | schema version |
| `socket` | path | `/run/anth/engine.sock` | the Unix socket |
| `listen` | address | unset | TCP is off by default |
| `state` | path | `/var/lib/anth` | apps, grants, composition, credentials |
| `log.level` | enum | `info` | `error`, `warn`, `info`, `debug` |
| `log.format` | enum | `text` | `text` or `json` |
| `auth.session-ttl` | duration | `12h` | TCP only; the socket uses peer credentials |
| `apps.require-signature` | bool | `true` | `false` only in development |
| `apps.allow-unverified-assets` | bool | `false` | development only: skips UI asset hashes, never signatures or grants |
| `apps.max-bundle-bytes` | int | `33554432` | the install body cap |
| `limits.fuel` | int | per app | default when the manifest omits it |
| `limits.memory-pages` | int | `256` | |
| `limits.wall-ms` | int | `5000` | backstop |
| `backend` | enum | `cranelift` | reserved: the Pulley profile selects `pulley` |

Rules:

- **Debug builds default open, release builds closed.** A debug binary uses
  relative paths and permissive switches so a developer does not need root; a
  release binary uses absolute paths and refuses the permissive ones.
- **The file is created, not required.** The first boot writes the release
  defaults with their keys commented, so the operator sees what can be changed.
- **`anth config set` edits one dotted key and keeps the comments.** That is part
  of why the format is TOML (`D-Format`).
- **Secrets never live here.** Operator credentials are in
  `state/credentials.txt`; trusted keys are in the engine's keyring.

### 6.8 State and audit

Everything the engine owns lives under `state` (§6.7):

```
state/
  apps/<app-id>/            bundle contents (verified before visible) + descriptor.toml (engine-written: enabled, alias, signer, tier, installed at)
  grants/<app-id>.toml      the frozen grant of each installed app
  composition.toml          the composition document (§8.1)
  credentials.txt           operator name and password, written once (§6.5)
  keys/<fingerprint>.toml   trusted third-party keys (§9.1)
  data/<app-id>/kv.json     the app's `anth:store/kv` namespace (host-written)
  audit.jsonl               append-only audit
```

Directories are `0700` and files `0600`. The engine creates the tree on first boot,
and refuses to start if one of these paths is a symlink.

**Audit entries.** One JSON object per line, append-only, never rewritten:

```json
{"at":"2026-11-02T18:40:12Z","actor":"operator-3f2a","action":"app.approve",
 "app":"org.anth.hello","result":"ok","detail":{"risk":"read"}}
```

`at` is UTC, `actor` is the operator name or `system`, and `action` is one of
`app.install`, `app.upgrade`, `app.approve`, `app.revoke`, `app.enable`,
`app.disable`, `app.remove`, `compose`, `key.add`, `key.remove`, `acknowledge`,
`deny`, `session.login`. `detail` carries what the action needs: the risk, the
dependents broken, the capability denied.

A denial is audited even when nothing changed. The record of what an app tried is
the point of having one.
