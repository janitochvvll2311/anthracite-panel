# Anthracite v0.5.0 — an engine that composes apps

Status: **accepted**. This document replaces the v0.3.0 design and the v0.4.0
capability plan as the architecture of record. Those remain useful as a *feature*
reference (frozen at tag `v0.4.0-freeze`) and as a catalogue of mistakes to
avoid, not as a template.

Decisions taken during review are recorded in §16. They are binding: a change to
one of them is a design change, not an implementation detail.

---

## 1. The thesis

> Anthracite is not a panel that grew features. It is a small engine that
> composes apps into a running system, and every feature — including the panel
> itself — is one of those apps.

The engine owns mechanism: boot, supervision, transport, authority, storage
namespaces, audit. Nothing in the engine knows what a file is, what a terminal
is, or how a firewall rule looks. Those are policies, and policies are apps.

The product we are building in v0.5.0 is therefore not a feature set. It is the
**scaffold**: the contract, the resolver, the runtime, the broker and the
conformance kit that let somebody else write an app we have never seen and have
it run, safely, without editing our code.

**v0.5.0 stops at M3.** It ships the platform and the apps that prove it
(`shell`, `hello`). The feature apps that existed in v0.4.0 return in v0.6.0 as
system apps. This is deliberate: the platform is judged by its contract, not by
its catalogue.

Success is measurable:

> **The acid test.** An app is written outside the engine source tree, imports a
> filesystem capability, contributes one route and one UI slot. It is installed,
> approved and composed. It appears in the shell and answers over HTTP and the
> CLI — with **zero changes to any engine source file**.

In v0.5.0 we execute the acid test ourselves. In v0.6.0 somebody who is not us
must be able to.

---

## 2. Why restart

v0.4.0 proved the hard parts: one HTTP dialect over socket and TCP, typed
exports and imports, frozen grants, a wasm runtime with a capability bridge, and
a signed bundle format. It also demonstrated the failure mode we are escaping:
every feature edited the host crate, the router, the CLI and the shell, because
features lived *inside* the product instead of *on* it.

v0.5.0 keeps the *lessons* and discards the *structure*:

- keep: one HTTP dialect, typed contracts, frozen grants, signature and hash
  verification, hash-pinned assets, the "manifest is a proposal, the grant is the
  contract" rule;
- discard: feature code in the host, a monolithic `AppService`, a router that
  knows feature routes, a shell that knows feature sections, a home-grown type
  language and a hand-rolled ABI.

---

## 3. Principles

1. **Mechanism in the engine, policy in apps.** If it can be expressed as an app,
   it must be.
2. **One contract, no side doors.** There is no privileged path around the app
   contract — not for our own apps, not for the shell.
3. **The manifest is a proposal; the grant is the contract.** Runtime behaviour is
   validated against the frozen grant, and the component's real type must match
   it before anything is linked.
4. **Composition is data.** What runs, and who provides what, is a file an
   operator can read, diff and review — not code.
5. **The type system is WIT.** We do not invent a second one. Interfaces are
   versioned, exact and checked structurally.
6. **Interfaces are small and single-purpose.** The grant scopes at interface
   granularity, so a god-interface would be a god-grant.
7. **The native capability set is closed.** The engine provides a small, fixed
   list of environment interfaces (§9.2). Growing that list is a design change.
8. **Failure is local.** One broken app degrades itself and its dependents; it
   never takes the engine down.
9. **The platform is done when adding an app needs no engine change.**
10. **Everything is observable.** Composition state, grants and denials are
    queryable and auditable without reading logs.

---

## 4. Vocabulary

| Term | Meaning |
| --- | --- |
| **Engine** | The daemon. Mechanism only: config, transport, supervision, runtime, broker, store, audit. |
| **App** | A unit of policy and product: a bundle with a manifest, a component and an authority surface. |
| **Bundle** | The signed zip: manifest, component, WIT, optional service binary, UI assets. |
| **Manifest** | The app's *proposal*: an index of interfaces, imports, routes, slots and limits. Carries no types. |
| **Interface** | A WIT interface: `namespace:package/interface@semver`. The unit of capability, versioning and grants. |
| **Export** | An interface the app implements. |
| **Import** | An interface the app requires. Must match a provider exactly, by name and type. |
| **Grant** | The frozen, operator-approved authority derived from a manifest. The runtime contract. |
| **Route** | An HTTP binding from `METHOD /path` to `interface#function`. |
| **Slot** | A named place in the shell where an app may render UI. |
| **Composition** | The declarative answer to "what is installed, what is composed, who provides what". |
| **Surface** | The runtime result of a composition: routes, slots, CLI verbs, links. |
| **Tier** | `engine`, `system`, `third-party`. Determines what authority may be requested at all. |

---

## 5. Architecture at a glance

```
                     ┌───────────────── engine (mechanism) ─────────────────┐
   browser  ────────►│ router · sessions · asset host                        │
   cli      ────────►│    │                                                  │
   (HTTP)            │  composer ── registry ── grant store ── audit          │
                     │    │                                                  │
                     │  supervisor ── runtime(wasmtime) ── broker ── store    │
                     └────┼──────────────────────────────────────────────────┘
                          │  typed component links + host interface calls
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

---

## 6. The engine

### 6.1 Subsystems

| Subsystem | Contract |
| --- | --- |
| **boot** | Read config, open the store, load the composition, start apps in dependency order, expose the surface. No feature logic. |
| **router** | One HTTP surface over the Unix socket and TCP: `/` (bootstrap document), `/api/engine/*`, `/api/apps/<id>/*`. Auth, origin and CSP live here and only here. |
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
| render or know sections of the UI | the shell is an app; slots are data |
| hard-code a CLI verb per feature | CLI verbs are projected from WIT (§7.9) |
| grant authority at install time | approval is a separate, explicit act |
| trust the manifest at runtime | the grant is the contract |
| grow the native capability set casually | the set is closed by design (§9.2) |
| hand a filesystem path to an app | a path is ambient authority; state is an interface (§7.10) |

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
  pays: `anth-cli` is a thin HTTP client.

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

## 7. The app

### 7.1 Bundle layout

```
manifest.toml            # the proposal; the signable index
manifest.sig             # detached signature over manifest + WIT + hashes
wit/<interface>.wit      # one file per exported interface (the contract, readable)
wasm/<name>.wasm         # the component (required)
bin/<name>               # optional service binary (system tier only)
web/<name>.js            # optional UI module(s), hash-pinned by the manifest
```

Rules inherited from v0.4.0, kept because they worked: no absolute paths, no
`..`, no symlinks, a bounded entry count, a bounded size per file, every declared
hash verified before the bundle becomes visible.

### 7.2 Manifest

The manifest is an **index**, not a type definition. Types live in WIT, so the
manifest only says *which* interfaces, *where* they are and *how* they are
reached.

```toml
manifest_version = 2

[app]
id = "org.example.notes"
namespace = "example:notes"        # every exported interface lives under this
name = "Notes"
version = "0.1.0"
summary = "A tiny notes app."
license = "Apache-2.0"

[requires]
engine = ">=0.5,<0.6"

[wasm]
path = "wasm/notes.wasm"
sha256 = "…"

# ---- what it offers ----
[[export]]
interface = "example:notes/list@1.0.0"
path = "wit/list.wit"
sha256 = "…"

# ---- what it needs ----
[[import]]
interface = "anth:fs/list@1.0.0"

[[import]]
interface = "anth:store/kv@1.0.0"

# ---- HTTP surface ----
[[route]]
method = "GET"
path   = "/notes"
call   = "example:notes/list@1.0.0#all"

# ---- UI surface ----
[[slot]]
id     = "shell.main"
tag    = "anth-notes"
module = "web/notes.js"
sha256 = "…"
label  = "Notes"

# ---- CLI surface ----
[[cli]]
call = "example:notes/list@1.0.0#all"
verb = "notes list"

# ---- limits ----
[limits]
fuel          = 10_000_000      # per invocation, per-instruction accounting
memory-pages  = 256
wall-ms       = 5_000           # backstop only, never the primary limit
bytes-total   = 8_388_608       # reserved: unused until streams land (v0.6.0)
bytes-per-sec = 1_048_576       # reserved: unused until streams land (v0.6.0)
```

Limits are a **budget**, not arguments scattered through calls: the runtime gets
one budget per instance and enforces it the same way whether the work is a single
call or a stream. The `bytes-*` keys exist now so that the enforcement path does
not change shape later.

A manifest is a *claim*. Nothing in it is trusted until the composer resolves it
and the operator approves the resulting grant.

### 7.3 WIT as the contract

Interfaces are WIT, in the canonical form:

```wit
package anth:fs@1.0.0;

interface list {
  use anth:error@1.0.0.{error};

  record entry {
    name: string,
    kind: enum { file, dir, link },
    size: u64,
    modified-ms: u64,
  }

  all: func(dir: string) -> result<list<entry>, error>;
}
```

**Identity.** Interfaces are `namespace:package/interface@semver`. The `anth`
namespace is reserved for the engine; an app cannot claim it. App ids stay in
reverse-DNS (`org.example.notes`) because that is what DNS-shaped ids are for,
while the app's WIT namespace (`example:notes`) is declared in the manifest and
must be unique across a composition.

**Versioning is exact.** The component `Linker` resolves imports by name, so
`@1.0.0` and `@1.0.1` are different interfaces and do not link. This is not a
policy choice, it is what the tool does. To evolve an interface, a provider
exports the old and the new version side by side and consumers migrate on their
own schedule. No ranges, no negotiation, no silent widening.

**Files, not strings.** WIT lives in `wit/*.wit` inside the bundle, hashed in the
manifest and covered by the signature. It is a language with tooling: embedding
it in TOML would make it unformattable and unreviewable.

**Two sources, one truth.** The manifest declares imports and exports so the
composer can resolve *before* instantiating. The component embeds the real world.
The engine compares them and refuses the app on any difference (§7.12). The
manifest lets us plan; the component is what we check.

### 7.4 App kinds

One app may combine three implementations; the contract is the same for all of
them.

| Kind | Runs as | May request | Typical use |
| --- | --- | --- | --- |
| `wasm` | in-process component, sandboxed, fuel-metered | any granted interface | all logic, including heavy compute |
| `service` | supervised child process, fixed argv, no ambient env | `exec`/`socket`/`device` (system tier only) | the few operations that need the kernel: PTYs, `ip`, `nft`, `sfdisk` |
| `ui` | sandboxed iframe served by the engine | RPC to its own app only | rendering |

A `service` is an app, not an engine module: it has an id, a manifest, a grant, a
route and a limit. What it does *not* have is unrestricted host access — the
supervisor hands it a narrow, declared interface (a socket, a set of argv
templates, a device file) and nothing else.

### 7.5 The component world

An app's component has one world. A minimal one:

```wit
package example:notes@0.1.0;

world app {
  import anth:fs/list@1.0.0;
  import anth:store/kv@1.0.0;

  export list;
}
```

That is the entire ABI. There is no `anth_call`, no packed pointers, no JSON
envelope for host calls: values are lifted and lowered by the canonical ABI, and
every host import is an interface the broker implements.

Cross-app calls are component links: the composer satisfies `anth:fs/list@1.0.0`
with the instance of the app that exports it. No serialization, no router, no
broker.

### 7.6 Routes

A route is `METHOD /path -> interface#function`. The engine mounts them under
`/api/apps/<app-id>/`, so two apps may both declare `/status` without colliding
and an operator can tell from the URL whose code is answering.

```
GET /api/apps/org.example.notes/notes
```

The engine maps the HTTP request to function arguments using the WIT types:

- record fields become named query parameters (`GET`) or a JSON object body;
- `bytes` accepts a body up to the route's limit;
- `result` maps to the status codes of §7.11;
- a guest trap maps to `internal` with a distinct code, sanitized at the boundary.

Argument and reply values are type-checked at the canonical ABI boundary by
construction; there is no second validation pass to forget.

### 7.7 UI slots

The shell app declares slots; apps contribute modules. A slot contribution names
a tag, a module path and a hash. The engine serves the module from the bundle,
never from disk on demand, and the shell mounts it in a sandboxed iframe with a
`postMessage` RPC scoped to that app's routes only.

Slots in v0.5.0: `shell.nav`, `shell.main`, `shell.settings`. A slot that nobody
fills is simply empty — the shell has no hard-coded section list.

### 7.8 Events

**Not in v0.5.0.** The model is agreed — an event is a typed notification
interface, the graph is fixed at compose time, and there is no dynamic topic
bus — but the mechanism needs either component async streams (v0.6.0, when WASI
0.3 is in play) or generated glue that bridges dynamic WIT types. Neither
belongs in a milestone whose job is to prove routes and slots.

What v0.5.0 does: reserves the `anth:notify` namespace, and lets an app refresh
by being called. A UI that needs to poll calls its own app's route; the shell
does not push.

The intended shape, for the record:

```wit
package anth:fs@1.0.0;

interface changed {
  record event { path: string, kind: enum { created, modified, removed } }
  on-change: func(event: event);
}
```

Consumers import it, the publisher emits it, the engine fans out. No strings, no
payload schemas, no subscription grants.

### 7.9 CLI and clients

One generator, three outputs, all derived from WIT:

- **CLI verbs** — function parameters become flags (named from the WIT record
  fields, typed from WIT).
- **Rust client** — one method per exported function.
- **TypeScript client** — the same, for the shell and app UIs.

A manifest may add a `[[cli]]` alias so system apps read well (`anth app fs ls`
instead of a mechanical path), but nobody hand-writes a parser. The v0.4.0 lesson
is that hand-written CLI and hand-written clients drift from the server; the fix
is generation, not discipline.

### 7.10 State

Two hard rules:

1. **No app ever receives a filesystem path.** A path is ambient authority in
   disguise.
2. **No app may assume a fresh instance per call.** State lives in
   `anth:store/*`; the instance is a runtime detail. This is the one assumption
   that would be expensive to retrofit: instances become long-lived the moment a
   stream exists, and an app that relied on a clean instance would silently
   change behaviour when that happens.

| Interface | Shape | Use |
| --- | --- | --- |
| `anth:store/kv@1.0.0` | `get`/`set`/`delete`/`list`, namespaced per app | small state |
| `anth:store/blobs@1.0.0` | `put`/`get` by hash, quota'd | large data |

Both are host interfaces (§9.2), so they are audited and quota'd by the broker.
In v0.5.0 blobs travel as `bytes` with a cap; when streams land they become
`stream<u8>` without changing any app's contract.

Durability and migrations are the app's business: it declares a schema version
and migrates on first boot of the new version. The engine never interprets app
state.

### 7.11 Errors

One interface, used by every host call and every route:

```wit
package anth:error@1.0.0;

interface kinds {
  variant error {
    denied(string),
    unavailable(string),
    invalid(string),
    not-found(string),
    too-large(u64),
    internal(string),
  }
}
```

The engine fixes the HTTP mapping, so HTTP, CLI and both clients cannot disagree:

| Variant | HTTP | Typical cause |
| --- | --- | --- |
| `denied` | 403 | capability not granted |
| `unavailable` | 503 | provider missing or degraded |
| `invalid` | 400 | arguments refused by the app |
| `not-found` | 404 | unknown resource or route |
| `too-large` | 413 | payload over the route limit |
| `internal` | 500 | app bug or trap; message sanitized |

`internal` messages are scrubbed at the HTTP boundary: a third-party app never
gets to leak host paths or engine internals to a browser.

### 7.12 Enforcement: the world must match the grant

This is the security core, and it happens before a single instruction runs:

1. Parse the WIT the grant froze and build the expected world.
2. Read the component's real type with `Component::component_type()`.
3. Compare imports and exports: names, versions and structural types. Any
   difference, including an extra import, is a refusal.
4. Link only the imports the grant lists, and only to the providers the
   composition names.
5. Only then instantiate.

A component that lies in its manifest does not start. A component that imports
something it was not granted does not start. A component whose types drifted from
the approved WIT does not start. All three are conformance tests, not review
practices.

---

## 8. Composition

### 8.1 The file

```toml
version = 1

[[profile]]
id   = "default"
apps = ["org.anth.shell", "org.anth.fs", "org.example.notes"]

# Explicit provider choices. Omit to accept the unique provider, if there is one.
[provide]
"anth:fs/list@1.0.0"   = "org.anth.fs"
"anth:fs/read@1.0.0"   = "org.anth.fs"
"anth:env/info@1.0.0"  = "@engine"

[surface]
root = "org.anth.shell"      # owns "/" and the slot definitions
```

### 8.2 Resolution

Deterministic, in this order:

1. Load the composition; reject unknown keys (typos are errors).
2. Resolve each app id to exactly one installed manifest; `id@version` pins.
3. Union every import of every composed app, keyed by interface id.
4. For each import, pick a provider: explicit `[provide]` wins; otherwise a
   unique composed exporter; otherwise `@engine` if the interface is in the
   native set; otherwise **unresolved**.
5. Check that the provider's granted export and the consumer's granted import are
   the same interface, exactly.
6. Order apps so providers boot before consumers. Cycles are a composition error:
   typed links cannot be cyclic.
7. Build the surface: routes, slots, CLI verbs, links.
8. Start apps in order with per-app timeouts. A failure marks that app and its
   dependents `degraded` and the boot continues.

### 8.3 Rules

- **One provider per interface.** Two composed providers of the same interface id
  is a configuration error. Silent "first one wins" is how platforms rot.
- **No implicit providers.** `@engine` only for the closed native set.
- **No partial composition.** If an import is unresolved, the consumer does not
  start; it does not start with a stub.
- **Composition does not grant.** Composing an unapproved app is an error that
  names the missing approval.
- **Exact means exact.** No minor-version satisfaction, no structural coercion.

### 8.4 Linking model

- **App → app**: a typed component link, created by the composer only when the
  grant lists that import. No broker, no serialization, no per-call quota.
- **App → host**: an interface call implemented by the broker, which validates
  against the grant, enforces quotas and records denials.

The rule, stated precisely: *the broker is the only exit from the composition;
inside it, authority was exercised when the composition was built.* This is why
the grant must be checked against the component's real world (§7.12): a link is
authority.

Consequence we accept: a trapping provider traps its linked consumers, and
cross-app calls are not individually audited. If that ever needs to change, the
change is to the linker, not to every app.

### 8.5 Degraded mode

A missing or failed provider yields a typed `503` on its consumers' routes:

```
{"error":"unavailable","message":"anth:fs/list@1.0.0 has no provider in this composition","app":"org.example.notes"}
```

The engine stays up, the shell still renders, the operator sees which app is
degraded and why. `anth engine composition` prints the full state.

### 8.6 Recompose

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
not a capability we hand out with a checkbox.

### 9.2 The closed native capability set

The engine provides exactly these interfaces, because they are mechanism-adjacent
and every app needs them:

```wit
anth:env/info@1.0.0      host facts (hostname, os, arch, uptime, cpu, memory)
anth:env/time@1.0.0      clock, monotonic and wall
anth:env/log@1.0.0       structured logging into the engine log
anth:env/random@1.0.0    cryptographically secure bytes
anth:store/kv@1.0.0      the app's own key/value namespace
anth:store/blobs@1.0.0   the app's own content-addressed blobs
```

Anything else — files, processes, terminals, network, storage, accounts, the app
manager itself — is an app that another app imports.

### 9.3 Grants and approval

A grant freezes: app id, version, manifest hash, signature, WIT hash, each
exported interface, each imported interface with its resolved provider, the risk
level, the approval levels the operator presented, who approved and when.

Grants scope **per interface**, which is why §3.6 insists on small interfaces. A
grant for `anth:fs/read@1.0.0` covers the functions that interface declares and
nothing else.

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

- call an interface it was not granted;
- see another app's state, cache or blobs;
- receive a filesystem path;
- reach the engine's socket, the panel's socket, or the network, unless a granted
  interface says so;
- mutate its own grant;
- change its exports without a new approval, because the WIT hash is in the grant.

---

## 10. Workspace

```
Cargo.toml
wit/                  canonical host interfaces: anth:env, anth:store, anth:error
crates/
  anth-contract/      manifest, composition, grants, errors (no I/O, no runtime)
  anth-engine/        boot, config, router, supervisor, store, audit
  anth-runtime/       the only crate that names wasmtime; component world checks
  anth-broker/        host interface dispatch, quota enforcement, denials
  anth-compose/       registry, grant store, resolver, surface projection
  anth-cli/           the `anth` binary: engine control + generated app verbs
  anth-client/        generated HTTP client
apps/
  shell/              the reference UI app: nav, main, settings slots
  hello/              the smallest component: one export, one import, one route
  fs/ terminal/ proc/ net/ storage/ accounts/ manager/     (v0.6.0)
frontend/
  anth-sdk/           Lit + bridge: slots, RPC, i18n
  anth-client/        generated TypeScript client
  anth-shell/         the shell app build
docs/
  DESIGN.md           this file
  contracts/          manifest, composition, error mapping
tests/
  conformance/        the kit a third party runs against their bundle
  fixtures/           golden compositions: cycles, duplicates, missing providers
```

`anth-contract` has no dependencies on the rest. Everything else depends on it,
and nothing depends on the engine. That direction is the architecture.

---

## 11. The scaffold: what we build

Milestones are ordered so each one is demonstrable on its own.

### v0.5.0 — the platform (M0–M3)

**M0 — Kernel (no features)**

- Workspace, `anth-contract`, config, logging, one router over UDS + TCP.
- `/api/engine/health`, `/api/engine/version`, `/api/engine/composition`.
- Session auth, origin checks, CSP — in the router, once.
- Supervisor skeleton with the instance registry, and orderly shutdown.
- The `Runtime` trait with a working fake backend, so engine tests never need
  wasmtime and nothing above the trait learns its semantics.
- Toolchain gate: `wit-bindgen` builds a component and wasmtime instantiates it,
  offline, with the pinned toolchain.
- Async probe, recorded and not shipped: build a component with a `stream` under
  WASI 0.3 and run it on the pinned wasmtime. Its result is what M5 designs the
  PTY against.

**Done when:** `anth engine run` boots with an empty composition, `anth engine
doctor` is green, and contract tests cover config and error shapes.

**M1 — Composition**

- Registry: install, list, verify, remove; zip path safety, signatures, hashes,
  WIT validation.
- Manifest v2 parser, interface catalog, risk computation.
- Grant store and `anth app approve` with the `--accept-*` levels.
- Resolver with the eight steps of §8.2, plus typed errors for cycle, duplicate
  provider, unresolved import and unapproved app.
- `anth compose` and degraded mode.

**Done when:** fixture compositions produce golden surfaces, and every failure
mode in `tests/fixtures/` yields its specific typed error.

**M2 — First app**

- Component runtime with fuel, memory and wall backstop.
- World-vs-grant validation (§7.12) and the three refusal tests.
- Host interfaces `anth:env/*` and `anth:store/kv`.
- Routes to `interface#function`, HTTP mapping per §7.11.
- `apps/hello`: one exported interface, one imported interface, one route, CLI
  verb generated from WIT.

**Done when:** `hello` answers over HTTP and CLI, and a deliberately broken
component (extra import, drifted type, fuel exhaustion, denial) yields the right
typed error instead of a crash.

**M3 — Shell and slots**

- `apps/shell`: declares `shell.nav`, `shell.main`, `shell.settings`; renders
  contributions; owns `/` behind the engine's bootstrap document.
- Asset serving from bundles, hash-pinned, CSP-clean.
- Iframe bridge and `postMessage` RPC restricted to the owning app.
- `anth-sdk`, and the TypeScript client generated from WIT.

**Done when:** `hello` renders in a slot, and the shell lists a composed app it
was never compiled against. **That is the v0.5.0 acid test.**

### v0.6.0 — the proof (M4–M5)

**M4 — System apps**

`fs`, `terminal`, `proc`, `net`, `storage`, `accounts`, `manager`. Each is a
bundle with a manifest; each is removable; none requires an engine edit. Heavy
compute (archives, previews, hashing) stays in wasm.

**Done when:** a composition of system apps reproduces the v0.4.0 feature
baseline, and removing `net` leaves a working system with that section gone — not
a broken shell.

**M5 — Third party**

- Streams, designed against the terminal rather than a toy. Backpressure,
  cancellation, half-close and error-after-partial-data each need a conformance
  test or the feature does not ship.
- Typed events (§7.8) on top of those streams, as `subscribe() -> stream<event>`.
- Quotas per stream (bytes/s and total) enforced from the budget.
- Conformance kit published; `anth-conformance` runs against a bundle offline.
- Audit report, approval/diff UI in the shell's settings slot.
- Sandbox hardening for services; decision on shipping the Pulley profile.

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

- **Contract tests** for manifests, interfaces, grants, composition and error
  shapes, including property tests over path safety, manifest parsing and
  resolver determinism.
- **World-vs-grant tests**: extra import, missing export, drifted type, wrong
  version — each must refuse with its own error and no instantiation.
- **Golden compositions**: fixtures asserting a full surface (routes, slots,
  order, degraded set) and a specific error for the invalid ones.
- **Component conformance guests**: small components for each failure path —
  trap, fuel exhaustion, denial, oversized payload.
- **Golden HTTP traces**: the surface's responses, including degraded 503s.
- **The conformance kit** is the same suite a third party runs against their
  bundle. If our tests and their tests are different, the platform is not real.

Two rules for the suite:

- **Assert outputs, never durations.** Fuel is per-instruction and portable
  between backends; the clock is not.
- **The backend is not a contract.** Nothing asserts Cranelift-specific
  behaviour. If the Pulley profile ever ships, the same suite must pass on it.

e2e (browser) tests exercise the shell and one contributed slot, not every
feature.

---

## 13. Migration notes (reference only)

Nothing is ported wholesale. `v0.4.0-freeze` is a library of decisions:

| Keep | Drop |
| --- | --- |
| one HTTP dialect, UDS + TCP | feature modules inside `anth-panel` |
| frozen grants, risk, approval levels | `AppService` doing install *and* serve |
| hash-pinned assets, signed bundles | v2/v3 dual manifest paths |
| argv-based `ip`/`nft`/`sfdisk` with `pid+start_ticks` guards | router that enumerates feature endpoints |
| `host.info` as a snapshot | a shell that hard-codes sections |
| per-export anchor dispatch | the home-grown type language and flat ABI 4 |

The v0.4.0 features are re-implemented as apps in v0.6.0, from their behaviour,
not by copying their code.

---

## 14. Risks

| Risk | Mitigation |
| --- | --- |
| The contract becomes a framework, and apps fight it. | Ship `hello` and `shell` first; every ergonomic complaint is a contract change, not a local workaround. |
| The component model moves under us (WASI 0.3 async landed in 2026). | Pin wasmtime; treat async as a v0.6.0 dependency, not a v0.5.0 one. |
| JIT requires executable memory. | Documented requirement; the Pulley profile is the escape hatch and the scaffolding stays. |
| Binary size grows with a wasm compiler. | Only the daemon pays; curated wasmtime features, `opt-level=s`, LTO, strip, measured in M0. |
| Exact interface versioning forces coordinated upgrades. | Providers export two versions during a transition; that is the documented path. |
| Dynamic WIT glue (events) is expensive to build. | Deferred to v0.6.0 with streams as the intended mechanism. |
| The pre-shaped runtime boundary turns out to be wrong. | A `Runtime` trait with a fake backend is exercised by engine tests from M0, and the M0 async probe checks the real toolchain before M5 depends on it. |
| Services reintroduce ambient authority. | Tier rules: third party never gets a service in v0.5.0 or v0.6.0. |
| We rebuild v0.4.0 feature-first and lose the platform. | M0–M3 gate M4: no system app starts before the acid test passes. |

---

## 15. Open decisions

| # | Decision | Recommendation | By |
| --- | --- | --- | --- |
| D1 | Composition scope: system-wide vs. per-user | System-wide; per-user profiles later | M1 |
| D2 | Recompose: process restart vs. per-app restart | Process restart; revisit after M3 | M3 |
| D3 | PTY shape: two `stream<u8>` vs. a `resource` with methods for resize and signals | A resource, so control messages do not become protocol bytes | M5 |
| D4 | When to ship the Pulley profile | Only if a deployment target requires it; never as default | M5 |
| D5 | Signing keys for third-party apps | Distribution key for `system`, operator-trusted keys for the rest | M5 |
| D6 | Blob transfer shape in v0.5.0 | `bytes` with a cap; `stream<u8>` in v0.6.0 | M2 |

---

## 16. Decision log

Decisions taken during the v0.5.0 review. Binding until superseded.

| ID | Decision | Why |
| --- | --- | --- |
| R1.1 | v0.5.0 ends at M3; features return in v0.6.0 | The platform is judged by its contract, not its catalogue |
| R1.2 | WebAssembly Component Model + WIT from day one | One type system, real tooling, multi-language apps |
| R1.3 | Shell is an app; the engine serves only a bootstrap document | Otherwise the engine knows UI and the thesis is false |
| R1.4 | Iframe sandbox per app | The authority model matters more than theming convenience |
| R1.5 | Install, approve and compose are separate acts | Three different questions, three auditable states |
| D-Runtime | wasmtime + Cranelift, single backend | Reference implementation; lets wasm do heavy work |
| D-Pulley | Pulley documented, not shipped | W^X hosts are a deployment profile, not the default |
| D-Linking | Direct typed links app→app; broker only for host interfaces | Fighting the linker costs weeks and buys little |
| R2.1 | WIT ids for interfaces; reverse-DNS for apps; app WIT namespace declared | WIT cannot express DNS-shaped names |
| R2.2 | Exact interface versions | What the linker requires; evolve by exporting two versions |
| R2.3 | Manifest is an index; WIT in `wit/*.wit`, hashed and signed | WIT is a language, not a config string |
| R2.4 | Events are typed notifications, not a topic bus | Confirmed; deferred to v0.6.0 because the mechanism needs streams |
| R2.5 | CLI and clients generated from WIT | Hand-written surfaces drift; v0.4.0 proved it |
| R2.6 | State is interfaces only; never paths | A path is ambient authority |
| R2.7 | `anth:error@1` with a fixed HTTP mapping | One error model for HTTP, CLI and clients |
| R2.8 | The component world is validated against the grant before linking | A link is authority; a lying manifest must not run |
| D-Streams | Streams and typed events land in v0.6.0/M5, designed against the terminal; the runtime is pre-shaped now | A stream designed without a real consumer gets redesigned at the PTY anyway |

---

## Appendix A — the smallest vertical slice

What "one app composed over the engine" looks like end to end. It is the M3 demo,
and the shape every later app repeats.

```bash
# 1. install (content only; nothing is granted)
anth app install ./notes.zip
#    installed org.example.notes 0.1.0 (1 export, 2 imports, 1 route, 1 slot)
#    not approved: run `anth app approve org.example.notes`

# 2. approve (authority is frozen; risk is computed, not declared)
anth app approve org.example.notes --accept-read
#    approved org.example.notes 0.1.0 (risk read)
#    export  example:notes/list@1.0.0      wasm
#    import  anth:fs/list@1.0.0            -> org.anth.fs
#    import  anth:store/kv@1.0.0           -> @engine

# 3. compose (what runs is data)
anth compose --add org.example.notes

# 4. use it
curl --unix-socket /run/anth/engine.sock \
     http://localhost/api/apps/org.example.notes/notes?dir=/tmp
#    {"entries":[{"name":"…","kind":"dir","size":4096,"modified-ms":…}]}

anth app notes list --dir /tmp
#    same data, same types, another rendering
```

And the failures the platform must produce, not crash on:

```bash
# a dependency left the composition
anth compose --remove org.anth.fs
#    warning: org.example.notes is degraded: anth:fs/list@1.0.0 has no provider
#    the engine is up; 1 app degraded

curl --unix-socket /run/anth/engine.sock \
     http://localhost/api/apps/org.example.notes/notes
#    HTTP/1.1 503 Service Unavailable
#    {"error":"unavailable","message":"anth:fs/list@1.0.0 has no provider in this composition","app":"org.example.notes"}
```

```bash
# a component that asks for more than it was granted
anth app install ./sneaky.zip
#    refused: the component imports anth:fs/write@1.0.0 but the grant lists
#             only anth:fs/list@1.0.0
```
