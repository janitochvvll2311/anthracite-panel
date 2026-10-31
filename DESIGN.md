# Anthracite v0.5.0 — an engine that composes apps

Status: **proposal, not accepted**. This document replaces the v0.3.0 design and
the v0.4.0 capability plan as the architecture of record. Those remain useful as
a *feature* reference (frozen at tag `v0.4.0-freeze`) and as a catalogue of
mistakes to avoid, not as a template.

---

## 1. The thesis

> Anthracite is not a panel that grew features. It is a small engine that
> composes apps into a running system, and every feature — including the panel
> itself — is one of those apps.

The engine owns mechanism: boot, supervision, transport, authority, storage
namespaces, events, audit. Nothing in the engine knows what a file is, what a
terminal is, or how a firewall rule looks. Those are policies, and policies are
apps.

The product we are building in v0.5.0 is therefore not a feature set. It is the
**scaffold**: the contract, the resolver, the runtime, the broker and the
conformance kit that let somebody else write an app we have never seen and have
it run, safely, without editing our code.

Success is measurable:

> **The acid test.** A developer outside this repository writes a bundle that
> imports a filesystem capability, exposes one route, contributes one UI slot and
> publishes one event. They install it, approve it and compose it. The app
> appears in the shell, answers over HTTP and exposes a CLI verb — with **zero
> changes to any engine source file**.

If that sentence is true, v0.5.0 succeeded. If adding an app requires touching
the daemon, the router, the CLI and the shell, we have rebuilt v0.4.0 with new
names.

---

## 2. Why restart

v0.4.0 proved the hard parts: one HTTP dialect over socket and TCP, typed
exports and imports, frozen grants, a wasm runtime with a capability bridge, and
a signed bundle format. It also demonstrated the failure mode we are escaping:
every feature edited the host crate, the router, the CLI and the shell, because
features lived *inside* the product instead of *on* it.

The v0.4.0 daemon knows too much. Its `AppService` both installs bundles and
serves their anchors; its router both authenticates and enumerates feature
endpoints; its shell both renders the system and hard-codes the sections. Adding
"files" meant editing platform code. That is the thing we are removing.

v0.5.0 keeps the *lessons* and discards the *structure*:

- keep: one HTTP dialect, typed contracts, frozen grants, signature and hash
  verification, hash-pinned assets, the "manifest is a proposal, the grant is the
  contract" rule;
- discard: feature code in the host, a monolithic `AppService`, a router that
  knows feature routes, a shell that knows feature sections.

---

## 3. Principles

1. **Mechanism in the engine, policy in apps.** If it can be expressed as an app,
   it must be.
2. **One contract, no side doors.** There is no privileged path around the app
   contract — not for our own apps, not for the shell.
3. **The manifest is a proposal; the grant is the contract.** Runtime behaviour is
   validated against the frozen grant, never against the manifest currently on
   disk.
4. **Composition is data.** What runs, and who provides what, is a file an
   operator can read, diff and review — not code.
5. **Authority is explicit and typed.** An app receives exactly the imports it was
   granted, at exactly the versions it was granted, with exactly the schemas it
   was granted.
6. **The native capability set is closed.** The engine provides a small, fixed
   list of environment capabilities (§9.2). Growing that list is a design
   change, not a commit.
7. **Failure is local.** One broken app degrades itself and its dependents; it
   never takes the engine down.
8. **The platform is done when adding an app needs no engine change.**
9. **Everything is observable.** Composition state, grants and denials are
   queryable and auditable without reading logs.

---

## 4. Vocabulary

| Term | Meaning |
| --- | --- |
| **Engine** | The daemon. Mechanism only: config, transport, supervision, runtime, broker, store, bus, audit. |
| **App** | A unit of policy and product: a bundle with a manifest, an implementation and an authority surface. |
| **Bundle** | The signed zip: manifest, wasm module, optional service binary, UI assets. |
| **Manifest** | The app's *proposal*: exports, imports, routes, slots, events, limits. |
| **Export** | A typed, versioned entry point an app offers: `id@version` plus input/output types. |
| **Import** | A typed, versioned entry point an app requires. Must match a provider exactly. |
| **Grant** | The frozen, operator-approved authority derived from a manifest. The runtime contract. |
| **Route** | An HTTP binding from `METHOD /path` to an export. |
| **Slot** | A named place in the shell where an app may render UI. |
| **Composition** | The declarative answer to "what is installed, what is composed, who provides what". |
| **Surface** | The runtime result of a composition: routes, slots, CLI verbs, event wiring. |
| **Tier** | `engine`, `system`, `third-party`. Determines what authority may be requested at all. |

---

## 5. Architecture at a glance

```
                     ┌───────────────── engine (mechanism) ─────────────────┐
   browser  ────────►│ router · sessions · asset host                        │
   cli      ────────►│    │                                                  │
   (HTTP/SSE/WS)     │  composer ── registry ── grant store ── audit          │
                     │    │                                                  │
                     │  supervisor ── runtime(wasm) ── broker ── store ── bus │
                     └────┼──────────────────────────────────────────────────┘
                          │  typed calls only
        ┌─────────────────┼───────────────┬──────────────────┐
   org.anth.shell   org.anth.fs     org.anth.terminal   third-party app
      (ui app)     (system app)       (system app)       (wasm + ui)
```

The lifecycle of one app:

```
bundle ──install──► installed ──approve──► granted ──compose──► running
          (verify)              (freeze)              (wire)
```

Four separate questions, four separate answers, four separate files on disk:

| Question | Answer | Owner |
| --- | --- | --- |
| What content exists? | installed bundles under `apps/` | registry |
| What may it do? | frozen grants under `grants/` | grant store |
| What is active? | `composition.toml` | operator |
| What is reachable now? | the surface | composer |

v0.4.0 partially merged these (an install could create a grant, a grant implied
execution). v0.5.0 keeps them apart, because "installed but not approved",
"approved but not composed" and "composed but degraded" are legitimate states an
operator must be able to see.

---

## 6. The engine

### 6.1 Subsystems

| Subsystem | Contract |
| --- | --- |
| **boot** | Read config, open the store, load the composition, start apps in dependency order, expose the surface. No feature logic. |
| **router** | One HTTP surface over the Unix socket and TCP: `/` (shell asset), `/api/engine/*`, `/api/apps/<id>/*`. Auth, origin and CSP live here and only here. |
| **registry** | Install, list, verify, remove bundles. Verifies zip paths, signatures, hashes and manifest shape. Never grants. |
| **composer** | Resolve `composition.toml` + installed manifests into a surface. Deterministic. Reports unresolved/duplicate/degraded. |
| **supervisor** | Own the process tree: wasm instances in-process, service apps as children. Restart policy, timeouts, resource limits, teardown. |
| **runtime** | Execute wasm guests: instantiate, honour fuel/memory limits, expose the ABI, mediate every host call. |
| **broker** | The only path from app code to anything outside the app. Validates against the grant, enforces quotas, records denials. |
| **store** | Per-app namespaces: `state/` (durable), `cache/` (evictable), `blobs/` (quota'd). No app sees another's. |
| **bus** | Typed topics. Publish and subscribe require grants. Bounded queues, explicit overflow errors. |
| **audit** | Append-only record of install/approve/compose/enable/disable/denials. |

### 6.2 What the engine must not contain

This list is the guardrail. A pull request that violates it is a design change,
not a feature.

| The engine must not… | Because |
| --- | --- |
| know about files, terminals, processes, network, storage or accounts | those are apps (`org.anth.*`) |
| enumerate feature endpoints in the router | routes come from the surface, computed at compose time |
| render or know sections of the UI | the shell is an app; slots are data |
| hard-code a CLI verb per feature | CLI verbs are projected from exports (§6.9) |
| grant authority at install time | approval is a separate, explicit act |
| trust the manifest at runtime | the grant is the contract |
| grow the native capability set casually | the set is closed by design (§9.2) |

---

## 7. The app

### 7.1 Bundle layout

```
manifest.toml            # the proposal; single signable file
manifest.sig             # detached signature over the manifest set
wasm/<name>.wasm         # logic (required for a wasm export)
bin/<name>               # optional service binary (system tier only)
web/<name>.js            # optional UI module(s), hash-pinned by the manifest
```

Rules inherited from v0.4.0, kept because they worked: no absolute paths, no
`..`, no symlinks, a bounded entry count, a bounded size per file, every declared
hash verified before the bundle becomes visible.

### 7.2 Manifest

```toml
manifest_version = 1

[app]
id = "org.example.notes"
name = "Notes"
version = "0.1.0"
summary = "A tiny notes app."
license = "Apache-2.0"

[requires]
engine = ">=0.5,<0.6"

# ---- implementation ----
[wasm]
path = "wasm/notes.wasm"
sha256 = "…"
abi = 1

# ---- what it offers ----
[[export]]
id = "org.example.notes.list"
version = 1
input  = { type = "record", fields = { dir = { type = "string" } } }
output = { type = "record", fields = {
  entries = { type = "list", of = { type = "string" } }
} }
[export.implementation]
kind = "wasm"
entry  = "anth_call"

# ---- what it needs ----
[[import]]
id = "org.anth.fs.list"
version = 1
input  = { type = "record", fields = { path = { type = "string" } } }
output = { type = "record", fields = {
  entries = { type = "list", of = { type = "string" } }
} }

# ---- HTTP surface ----
[[route]]
method = "GET"
path   = "/notes"
export = "org.example.notes.list"

# ---- UI surface ----
[[slot]]
id     = "shell.main"          # contributed by the shell app
tag    = "anth-notes"
module = "web/notes.js"
sha256 = "…"
label  = "Notes"

# ---- events ----
[[publish]]
topic   = "org.example.notes.changed"
payload = { type = "record", fields = { id = { type = "string" } } }

[[subscribe]]
topic = "org.anth.fs.changed"

# ---- limits ----
[limits]
fuel         = 10_000_000
memory_pages = 256
wall_ms      = 5_000
```

A manifest is a *claim*. Nothing in it is trusted until the composer resolves it
against the catalog and the operator approves the resulting grant.

### 7.3 Types

One type language, used identically in manifests, grants, calls and errors:

```
string · int · float · bool · bytes · timestamp · json
list<T> · map<T> · record{…} · enum[…] · nullable<T> · union<…>
```

Rules:

- **Exact matching.** A provider satisfies an import only when the types are
  structurally identical. No widening, no duck typing, no coercions.
- **Versioned, not negotiated.** `id@1` and `id@2` are different capabilities.
  An app may request both.
- **Validated twice.** The caller's argument is checked against the *grant's*
  frozen input type; the reply is checked against the frozen output type. A guest
  that answers outside its schema produces a typed internal error, never data.

### 7.4 App kinds

One app may combine three implementations; the contract is the same for all of
them.

| Kind | Runs as | May request | Typical use |
| --- | --- | --- | --- |
| `wasm` | in-process, sandboxed, fuel-metered | any granted capability | all logic |
| `service` | supervised child process, fixed argv, no ambient env | `exec`/`socket`/`device` (system tier only) | the few operations that need the kernel: PTYs, `ip`, `nft`, `sfdisk` |
| `ui` | sandboxed iframe served by the engine | RPC to its own app only | rendering |

A `service` is an app, not an engine module: it has an id, a manifest, a grant, a
route and a limit. What it does *not* have is unrestricted host access — the
supervisor hands it a narrow, declared interface (a socket, a set of argv
templates, a device file) and nothing else.

### 7.5 Entry points

The ABI stays deliberately small. Version 1:

```
anth_abi_version() -> i32
anth_alloc(len) -> ptr
anth_call(capability_ptr, capability_len, request_ptr, request_len) -> packed
```

`packed` is `(ptr << 32) | len` pointing at `{"status":u16,"body":any}` in guest
memory. A `service` app receives the same envelope over its declared socket, so
the same app logic can move between kinds without a rewrite.

Denial is a typed value, always:

```json
{"error":{"code":"denied","capability":"org.anth.fs.list","reason":"not granted"}}
```

Never a panic, never a silent `null`, never a partially-validated reply.

### 7.6 Routes

A route is `METHOD /path -> export`. The engine mounts them under
`/api/apps/<app-id>/`, so two apps may both declare `/status` without colliding
and an operator can tell from the URL whose code is answering.

```
GET  /api/apps/org.example.notes/notes
```

The anchor envelope handed to the export is fixed:

```json
{"method":"GET","path":"/notes","query":{…},"body":null}
```

and the reply is validated against the export's output type before it leaves the
engine.

### 7.7 UI slots

The shell app declares slots; apps contribute modules. A slot contribution names
a tag, a module path and a hash. The engine serves the module from the bundle,
never from disk on demand, and the shell mounts it in a sandboxed iframe with a
`postMessage` RPC scoped to that app's routes only.

Slots in v0.5.0: `shell.nav`, `shell.main`, `shell.settings`. A slot that nobody
fills is simply empty — the shell has no hard-coded section list.

### 7.8 Events

Topics are typed and namespaced (`org.example.notes.changed`). Publishing and
subscribing are separate grants. Delivery is at-least-once with a bounded queue;
overflow is a typed error to the publisher, not silent loss. A subscriber that
cannot keep up is marked degraded, not stopped.

### 7.9 CLI projection

Every export is reachable over HTTP. A *subset* is also reachable from the CLI,
derived from the export id — `org.anth.fs.list` becomes `anth app fs list` — so
the CLI cannot drift from the HTTP surface. Apps that want a friendlier verb
declare `cli = "fs ls"` on the export; they do not get to hand-write a parser.

The Rust and TypeScript clients are generated from the same registry of exports,
so CLI, HTTP and browser stay one surface with three renderings.

### 7.10 State

| Namespace | Durability | Quota | Owner |
| --- | --- | --- | --- |
| `state/` | durable | app-declared | the app, opaque to the engine |
| `cache/` | evictable | bounded | the engine may delete at will |
| `blobs/` | durable | app-declared | the app, content-addressed by the engine |

The engine stores the manifest and the grant; it never interprets app state. An
app that changes its state layout ships a migration and runs it on first boot of
the new version.

---

## 8. Composition

### 8.1 The file

```toml
version = 1

[[profile]]
id   = "default"
apps = ["org.anth.shell", "org.anth.fs", "org.anth.terminal", "org.example.notes"]

# Explicit provider choices. Omit to accept the unique provider, if there is one.
[provide]
"org.anth.fs.list"    = "org.anth.fs"
"org.anth.fs.write"   = "org.anth.fs"
"org.anth.host.info"  = "@engine"

[surface]
root = "org.anth.shell"      # owns "/" and the slot definitions
```

### 8.2 Resolution

Deterministic, in this order:

1. Load the composition; reject unknown keys (typos are errors).
2. Resolve each app id to exactly one installed manifest; `id@version` pins.
3. Union every import of every composed app.
4. For each import, pick a provider: explicit `[provide]` wins; otherwise a
   unique composed exporter; otherwise `@engine` if the id is in the native set;
   otherwise **unresolved**.
5. Check exact type equality and version equality; check the consumer's grant
   includes that import.
6. Order apps so providers boot before consumers. Cycles are allowed only through
   events; a call cycle is a composition error.
7. Build the surface: routes, slots, CLI verbs, event wiring.
8. Start apps in order with per-app timeouts. A failure marks that app and its
   dependents `degraded` and the boot continues.

### 8.3 Rules

- **One provider per import.** Two composed providers of the same `id@version` is
  a configuration error. Silent "first one wins" is how platforms rot.
- **No implicit providers.** `@engine` only for the closed native set.
- **No partial composition.** If an import is unresolved, the consumer does not
  start; it does not start with a stub.
- **Composition does not grant.** Composing an unapproved app is an error that
  names the missing approval.

### 8.4 Degraded mode

A missing or failed provider yields a typed `503` on its consumers' routes:

```json
{"error":{"code":"provider_unavailable",
          "message":"org.anth.fs.list has no provider in this composition",
          "app":"org.example.notes"}}
```

The engine stays up, the shell still renders, the operator sees which app is
degraded and why. `anth engine composition` prints the full state.

### 8.5 Recompose

v0.5.0 recomposes by restarting the engine process. It is boring, it is
predictable, and it keeps state assumptions honest. Per-app restart is a later
optimization, gated on the store being fully externalized (it is, by §7.10).

---

## 9. Authority

### 9.1 Tiers

| Tier | Source | May request |
| --- | --- | --- |
| `engine` | compiled in | nothing; it *provides* the native set |
| `system` | signed by the distribution key, shipped in `apps/` | wasm, ui, service, `exec`/`socket`/`device` |
| `third-party` | any other signer, operator-approved | wasm, ui only |

Third-party `service` apps are out of scope for v0.5.0. When they land, they get
their own design: a service is a host process, and "run arbitrary binaries" is
not a capability we can hand out with a checkbox.

### 9.2 The closed native capability set

The engine provides exactly these, because they are mechanism-adjacent and every
app needs them:

```
anth.env.info     host facts (hostname, os, arch, uptime, cpu, memory)
anth.env.time     clock, monotonic and wall
anth.env.log      structured logging into the engine log
anth.env.random   cryptographically secure bytes
anth.store.kv     the app's own key/value namespace
```

Anything else — files, processes, terminals, network, storage, accounts, the app
manager itself — is an app that another app imports.

### 9.3 Grants and approval

A grant freezes: app id, version, manifest hash, signature, each export with its
implementation hash, each import with its resolved provider, the risk level, the
approval levels the operator presented, who approved and when.

Risk is computed by the engine, never declared by the manifest:

| Level | Trigger (examples) |
| --- | --- |
| `read` | environment reads, clock, logs |
| `write` | mutating state, filesystem writes |
| `admin` | sockets, devices, network configuration |
| `host` | executing a binary, spawning a process |
| `package` | installing a system package |

Approval levels are derived from the risk of the whole set: `--accept-write`,
`--accept-admin`, `--accept-host`, `--accept-package`. An operator who skips a
level gets a refusal that names it.

### 9.4 What an app can never do

- call another app's export without a granted import;
- see another app's state, cache or blobs;
- read its own manifest from disk and act on it (the grant wins);
- reach the engine's socket, the panel's socket, or the network, unless a granted
  capability says so;
- mutate its own grant;
- publish or subscribe to a topic it was not granted.

---

## 10. Workspace

```
Cargo.toml
crates/
  anth-contract/      types, schemas, manifest, grants, composition, errors (no I/O)
  anth-engine/        boot, config, router, supervisor, store, bus, audit
  anth-runtime/       wasm host, ABI, envelope validation
  anth-broker/        capability dispatch, quota enforcement, denial recording
  anth-compose/       registry, grant store, resolver, surface projection
  anth-cli/           the `anth` binary: engine control + app verbs
  anth-client/        Rust HTTP/SSE/WS client
apps/
  shell/              the reference UI app: nav, main, settings slots
  hello/              the smallest wasm app: one export, one import, one route
  fs/ terminal/ proc/ net/ storage/ accounts/ manager/     (M4)
frontend/
  anth-sdk/           Lit + bridge: slots, RPC, i18n
  anth-client/        TypeScript client, generated from the same contract
  anth-shell/         the shell app build
docs/
  DESIGN.md           this file
  contracts/          manifest, ABI, composition, errors
tests/
  conformance/        the kit a third party runs against their bundle
  fixtures/           golden compositions: cycles, duplicates, missing providers
```

`anth-contract` has no dependencies on the rest. Everything else depends on it,
and nothing depends on the engine. That direction is the architecture.

---

## 11. The scaffold: what we build

Milestones are ordered so each one is demonstrable on its own. The platform is
M0–M3; M4 is the proof that it holds; M5 is the exit.

### M0 — Kernel (no features)

- Workspace, `anth-contract`, config, logging, one router over UDS + TCP.
- `/api/engine/health`, `/api/engine/version`, `/api/engine/composition`.
- Session auth, origin checks, CSP — in the router, once.
- Supervisor skeleton and orderly shutdown.

**Done when:** `anth engine run` boots with an empty composition, `anth engine
doctor` is green, and contract tests cover config and error shapes.

### M1 — Composition

- Registry: install, list, verify, remove; zip path safety, signatures, hashes.
- Manifest v1 parser, type checker, risk computation.
- Grant store and `anth app approve` with the `--accept-*` levels.
- Resolver with the eight steps of §8.2, plus typed errors for cycle, duplicate
  provider, unresolved import and unapproved app.
- `anth compose` and degraded mode.

**Done when:** fixture compositions produce golden surfaces, and every failure
mode in `tests/fixtures/` yields its specific typed error.

### M2 — First app

- Wasm runtime (fuel, memory, wall clock), ABI 1, envelope validation against the
  grant.
- Native capability set of §9.2.
- Routes, error mapping, HTTP surface for one app.
- `apps/hello`: one export, one import (`anth.env.info`), one route.

**Done when:** the acid-test app answers over HTTP through the engine, and a
deliberately broken guest (out-of-schema reply, fuel exhaustion, denial) yields
the right typed error instead of a crash.

### M3 — Shell and slots

- `apps/shell`: declares `shell.nav`, `shell.main`, `shell.settings`; renders
  contributions; owns `/`.
- Asset serving from bundles, hash-pinned, CSP-clean.
- Iframe bridge and `postMessage` RPC restricted to the owning app.
- `anth-sdk` and the TypeScript client generated from the contract.

**Done when:** `hello` renders in a slot and the shell lists a composed app it
was never compiled against.

### M4 — System apps (features return as apps)

`fs`, `terminal`, `proc`, `net`, `storage`, `accounts`, `manager`. Each is a
bundle with a manifest; each is removable; none requires an engine edit.

**Done when:** a composition of system apps reproduces the v0.4.0 feature
baseline, and removing `net` leaves a working system with that section gone —
not a broken shell.

### M5 — Third party

- Conformance kit published; `anth-conformance` runs against a bundle offline.
- Quotas, audit report, approval/diff UI in the shell's settings slot.
- Sandbox hardening: no ambient env for services, seccomp-style argv allowlists.

**Done when:** an app that lives outside this repository installs, composes,
renders and answers — the acid test, executed by someone who is not us.

### The platform's definition of done

1. `anth engine run` with no apps is a valid, healthy system.
2. Every feature is a bundle under `apps/`, removable from the composition.
3. No engine source file mentions a feature name.
4. A new app requires zero engine changes.
5. The conformance kit fails loudly on a bundle that lies.

---

## 12. Testing the platform

The platform is tested by *contract*, not by feature coverage.

- **Contract tests** for schemas, manifests, grants, composition and error shapes,
  including property tests over path safety, manifest parsing and resolver
  determinism.
- **Golden compositions**: a directory of small fixtures, each asserting a full
  surface (routes, slots, order, degraded set) and a specific error for the
  invalid ones.
- **ABI conformance guests**: hand-written WAT guests for each failure path —
  out-of-schema reply, fuel exhaustion, denial, oversized payload, panic.
- **Golden HTTP traces**: the surface's responses, including degraded 503s.
- **The conformance kit** is the same suite a third party runs against their
  bundle. If our tests and their tests are different, the platform is not real.

e2e (browser) tests exercise the shell and one contributed slot, not every
feature.

---

## 13. Migration notes (reference only)

Nothing is ported wholesale. `v0.4.0-freeze` is a library of decisions:

| Keep | Drop |
| --- | --- |
| one HTTP dialect, UDS + TCP | feature modules inside `anth-panel` |
| typed exports/imports, exact match | `AppService` doing install *and* serve |
| frozen grants, risk, approval levels | v2/v3 dual manifest paths |
| hash-pinned assets, signed bundles | router that enumerates feature endpoints |
| argv-based `ip`/`nft`/`sfdisk` with `pid+start_ticks` guards | a shell that hard-codes sections |
| `host.info` as a snapshot | a global catalog the engine must own |

The v0.4.0 features are re-implemented as apps in M4, from their behaviour, not
by copying their code.

---

## 14. Risks

| Risk | Mitigation |
| --- | --- |
| The contract becomes a framework, and apps fight it. | Ship `hello` and `shell` first; every ergonomic complaint is a contract change, not a local workaround. |
| Broker hops make the system slow. | Measure the acid test; if a call costs more than a millisecond, fix the ABI before M4, not after. |
| Wasm ABI churn (component model). | Keep the ABI behind `anth-runtime`; a second backend must not touch apps. |
| The iframe bridge hurts UX. | Slots are the shell's contract; if iframe isolation is too heavy, that is a design change with measurements attached. |
| Services reintroduce ambient authority. | Tier rules: third party never gets a service in v0.5.0. |
| Composition files confuse operators. | `anth compose --explain` prints the resolution; every error names the file, the app and the import. |
| We rebuild v0.4.0 feature-first and lose the platform. | M0–M3 gate M4: no system app starts before the acid test passes. |

---

## 15. Open decisions

Each needs a decision before its milestone; the recommendation is the default if
nobody objects.

| # | Decision | Recommendation | By |
| --- | --- | --- | --- |
| D1 | Wasm component model vs. flat ABI | Flat ABI behind `anth-runtime`; revisit after M3 | M2 |
| D2 | Manifest inline types vs. a separate schema dir | Inline: one signable file | M1 |
| D3 | Composition scope: system-wide vs. per-user | System-wide; per-user profiles later | M1 |
| D4 | Recompose: process restart vs. per-app restart | Process restart; revisit at M4 | M1 |
| D5 | UI isolation: iframe vs. shadow DOM | Iframe with a scoped bridge | M3 |
| D6 | Third-party `service` apps | Not in v0.5.0 | M5 |
| D7 | Event delivery guarantees | At-least-once, bounded, explicit overflow | M1 |
| D8 | CLI verb projection | Generated from exports; apps may alias | M2 |
| D9 | App state: engine KV vs. app-owned files | Both: KV for small state, dir for blobs | M2 |
| D10 | Is the shell an app or engine-served static? | An app — it is the proof the platform works | M3 |

---

## Appendix A — the smallest vertical slice

What "one app composed over the engine" looks like end to end. It is the M2
demo, and the shape every later app repeats.

```bash
# 1. install (content only; nothing is granted)
anth app install ./notes.zip
#    installed org.example.notes 0.1.0 (1 export, 1 import, 1 route, 1 slot)
#    not approved: run `anth app approve org.example.notes`

# 2. approve (authority is frozen; risk is computed, not declared)
anth app approve org.example.notes --accept-read
#    approved org.example.notes 0.1.0 (risk read)
#    export  org.example.notes.list@1  wasm
#    import  org.anth.fs.list@1        -> org.anth.fs

# 3. compose (what runs is data)
anth compose --add org.example.notes

# 4. use it
curl --unix-socket /run/anth/engine.sock \
     http://localhost/api/apps/org.example.notes/notes?dir=/tmp
#    {"entries":["…"]}

anth app notes list --dir /tmp
#    same data, one surface, two renderings
```

And the failure the platform must produce, not crash on:

```bash
anth compose --remove org.anth.fs
#    warning: org.example.notes is degraded: org.anth.fs.list has no provider
#    the engine is up; 1 app degraded

curl --unix-socket /run/anth/engine.sock \
     http://localhost/api/apps/org.example.notes/notes
#    HTTP/1.1 503 Service Unavailable
#    {"error":{"code":"provider_unavailable",
#              "message":"org.anth.fs.list has no provider in this composition",
#              "app":"org.example.notes"}}
```
