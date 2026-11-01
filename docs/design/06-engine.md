# 6. The engine

> Part of the [design index](../../DESIGN.md).

### 6.1 Subsystems

| Subsystem | Contract |
| --- | --- |
| **boot** | Read config, open the store, load the composition, start apps in dependency order, expose the surface. No feature logic. |
| **router** | One HTTP surface over the Unix socket and TCP: `/health`, `/api/health`, `/api/engine/*`, `/api/apps/<id>/*`, `/api/apps/<id>/assets/*`. No `/`. Auth, origin and CSP live here and only here. |
| **registry** | Install, list, verify, remove bundles. Verifies zip paths, signatures, hashes, manifest shape and WIT. Never grants. |
| **composer** | Resolve `composition.toml` + installed manifests into a surface. Deterministic. Reports unresolved/duplicate/degraded. |
| **supervisor** | Own the process tree and a registry of component instances with `acquire`/`release`/`evict`. Service apps as children. Restart policy, timeouts, resource budgets, teardown. |
| **runtime** | Behind one `async` trait: instantiate components with wasmtime, enforce the budget, validate the component world against the grant, satisfy host imports through the broker, link app imports. |
| **broker** | The only path from app code to the host. Validates against the grant, enforces quotas, records denials. |
| **store** | Per-app namespaces exposed as interfaces: `anth:store/kv`, `anth:store/blobs`. No paths, no sharing. |
| **audit** | Append-only record of install/approve/compose/enable/disable/denials. |

### 6.2 What the engine must not contain

This list is the guardrail. A pull request that violates it is a design change,
not a feature.

| The engine must not… | Because |
| --- | --- |
| know about files, terminals, processes, network, storage or accounts | those are apps (`org.anth.*`) |
| enumerate feature endpoints in the router | routes come from the surface, computed at compose time |
| **serve any UI, or own a slot** | the engine is headless; a panel hosts apps and declares slots |
| hard-code a CLI verb per feature | CLI verbs are projected from WIT (§7.9) |
| grant authority at install time | approval is a separate, explicit act |
| trust the manifest at runtime | the grant is the contract |
| grow the native capability set casually | the set is closed by design (§9.2) |
| hand a filesystem path to an app | a path is ambient authority; state is an interface (§7.10) |
| assume its client runs in the same process | a panel may be remote (§15) |

### 6.3 Runtime backend and deployment profiles

v0.5.0 ships **one** backend: **wasmtime with Cranelift**. It is the reference
implementation of the component model, it has fuel accounting, epoch-based
interruption and memory limits, and it is where every document, example and bug
fix lives. It is also the only backend that lets wasm do real work, which keeps
the "everything heavy can be an app" rule honest.

What that commits us to, explicitly:

- The daemon must be able to map **executable memory**. Hosts that forbid it
  (`deny_execmem`, grsecurity/PaX, a seccomp policy that rejects `PROT_EXEC`) are
  not supported by this build. This is a documented requirement, not a bug.
- The daemon binary grows by roughly the size of a wasm compiler. Only the daemon
  pays: the CLI in `sdk/rust/cli` is a thin client.

**Pulley is deferred, not rejected.** A hardened profile (`anth-engine-pulley`)
that interprets instead of generating code is documented for a later version. We
do not build it, test it or ship it in v0.5.0. What we do keep is the scaffolding
that makes it a build flag rather than a redesign:

- `anth-runtime` is the only crate that names `wasmtime`; nothing above it sees a
  runtime type.
- A single `Backend` enum is the only place a backend is chosen.
- Limits are expressed in **fuel and memory**, never in time, so a limit means
  the same thing on both backends.
- No precompiled artifact ever enters a grant, so an engine upgrade never
  invalidates an installed app.

The runtime boundary is one `async fn` behind a `Runtime` trait, and its second
implementation is a fake backend used by the engine's own tests. Nothing above
that trait depends on wasmtime's semantics. Together with the instance registry
(§6.1), that is what makes streams a later change *inside* `anth-runtime` instead
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
| `/api/engine/composition` | composition and surface | replace the document | — | — | — |
| `/api/engine/apps` | catalogue | — | — | — | — |
| `/api/engine/apps/{id}` | descriptor and grant | install or replace the bundle | `enabled`, alias | uninstall | — |
| `/api/engine/apps/{id}/grant` | frozen grant | approve with the accepted levels | — | revoke | — |
| `/api/engine/audit` | paginated log | — | — | — | — |
| `/api/engine/operator` | operator identity | — | change password | — | — |
| `/api/engine/session` | current session | — | — | log out | log in |
| `/api/engine/restart` | — | — | — | — | restart |
| `/api/apps/{id}/<path>` | per `[[route]]` | per `[[route]]` | per `[[route]]` | per `[[route]]` | per `[[route]]` |
| `/api/apps/{id}/assets/{*module}` | UI module | — | — | — | — |

Why these verbs:

- **`PUT` installs and approves.** A bundle and a grant are documents with a known
  identity: `PUT /apps/{id}` creates or replaces, and an upgrade is the same verb
  under the rule of §9.5. A manifest that declares a different id is `409`.
  Retrying is safe.
- **`PATCH` enables, disables and renames.** `{"enabled": false}` is a partial
  mutation of a resource, not an invented action endpoint.
- **`DELETE` removes and revokes.** `DELETE /apps/{id}` uninstalls;
  `DELETE /apps/{id}/grant` revokes the authority and keeps the content.
- **`PUT` replaces the whole composition**, not `POST /compose/add`. The
  composition is a document; adding and removing are client-side edits to it,
  guarded by `ETag` and `If-Match` so a concurrent edit cannot be lost.
- **`POST` only for restart.** It is a process transition, not a resource.

Method rules:

- An unsupported method is `405` with an `Allow` header, never a `404`.
- `OPTIONS` is answered by the engine for its own resources, with `Allow`. There
  is no CORS in v0.5.0: the browser always speaks to one origin, whether the panel
  mounted the router or proxied it.
- `HEAD` works on assets, and on app routes that declare it. The engine never
  fabricates a `HEAD` by calling a `GET`.
- `GET`, `HEAD` and `DELETE` carry no body; one is `400`. `PUT`, `PATCH` and
  `POST` have a per-resource cap.
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

App routes declare their verb, and the engine matches it exactly: no implicit
`HEAD`, no automatic `OPTIONS`, no `POST` standing in for `PUT`. `[[route]]`
accepts `GET`, `POST`, `PUT`, `PATCH`, `DELETE` and `HEAD`; `OPTIONS` belongs to
the engine.

A remote panel proxies the API unchanged — method, body and headers. A proxy that
rewrites methods makes the SDK lie.

### 6.5 Boot and lifecycle

Boot order, and the reason for it:

1. Config and logging.
2. Store, audit, and first-boot credentials.
3. Registry, grants and `composition.toml`.
4. Resolve the composition into a surface. A static miss is fatal (§8.5), and
   `anth doctor` explains it without a daemon.
5. Bind the socket. `/health` answers `200` from here on.
6. Supervisor: instantiate apps in dependency order with a per-app timeout.
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

**Mounting a panel.** The engine owns `/api/*`; a panel owns `/` and its own
assets. A panel that registers a colliding route fails to start: the last writer
never wins. The panel reads the composition through the API and never touches the
registry or the grants.

**Failure behaviour.**

| Failure | Behaviour |
| --- | --- |
| Invalid config | exit non-zero, naming the file and line |
| Unresolved import (static) | exit non-zero; `anth doctor` explains it offline |
| App traps (dynamic) | engine stays up, app degraded, consumers answer `503` |
| Remote panel cannot reach the engine | panel starts, renders a disconnected state, retries with backoff |

**Restart.** `POST /api/engine/restart` shuts down gracefully and re-execs the
same binary with the same arguments. In a linked panel that restarts the panel
too, which is what "restart the service" means. It answers `202 Accepted` with
`Location: /api/health` and never claims success before it is ready.

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
