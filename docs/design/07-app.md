# 7. The app

> Part of the [design index](../../DESIGN.md).

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

An app author gets the host interfaces from `sdk/rust/guest`, which packages
`anth:env`, `anth:store` and `anth:error`: depend on the crate and the `import`s
resolve. `engine/wit/` stays the source of truth and a CI check compares the two,
so the packaged copy cannot drift.

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
id     = "panel.main"
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

**Identity.** Interfaces are `namespace:package/interface@semver`. Namespaces are
owned by tiers:

| Namespace | Owner | Example |
| --- | --- | --- |
| `anth` | the engine | `anth:fs/list@1.0.0` |
| `anthracite` | first-party apps | `anthracite:hello/greeting@1.0.0` |
| anything else | third-party apps | `example:notes/list@1.0.0` |

An app cannot claim `anth` or `anthracite` unless its tier matches. App ids stay
in reverse-DNS (`org.example.notes`) because that is what DNS-shaped ids are for,
while the app's WIT namespace is declared in the manifest and must be unique
across a composition.

**Two version numbers, two meanings.** The package version follows the app
(`anthracite:hello@0.5.0`); the interface version is its own semver and changes
only when its types change (`greeting@1.0.0`). An app may ship 0.5.1 with an
unchanged `greeting@1.0.0`, and consumers do not notice.

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

**What the signature covers.** One detached signature over a canonical message:

```
"anth-manifest-v2\0"
for each file in sorted order, currently `manifest.toml` and `wit/*.wit`:
    name, "\0", length as little-endian u64, bytes
```

The wasm and the UI modules are not in the message; they are covered by their
hashes inside the manifest, and those hashes are covered by the signature. One
rule, written once, checked by one test with a fixed vector.

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
and an operator can tell from the URL whose code is answering. The accepted verbs
and the exactness of the match are fixed in §6.4.

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

A **panel** declares the slots it offers; apps contribute modules to them. A slot
contribution names a tag, a module path and a hash. The engine serves the module
from the bundle, never from disk on demand, and the panel mounts it in a sandboxed
iframe with a `postMessage` RPC scoped to that app's routes only.

The engine knows none of this: it exposes the slot contributions as part of the
composition view and serves the asset. Which slots exist is the panel's business.
Modules are served with `ETag` set to their pinned hash and `Cache-Control:
immutable` (§6.4), so a panel reload is a `304`, not a download.

Slots in v0.5.0, declared by the official panel: `panel.nav`, `panel.main`,
`panel.settings`. A slot that nobody fills is simply empty — a panel has no
hard-coded section list.

### 7.8 Events

**Not in v0.5.0.** The model is agreed — an event is a typed notification
interface, the graph is fixed at compose time, and there is no dynamic topic
bus — but the mechanism needs either component async streams (v0.6.0, when WASI
0.3 is in play) or generated glue that bridges dynamic WIT types. Neither
belongs in a milestone whose job is to prove routes and slots.

What v0.5.0 does: reserves the `anth:notify` namespace, and lets an app refresh
by being called. A UI that needs to poll calls its own app's route; the panel
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
- **TypeScript client** — the same, for panel and app UIs.

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

### 7.13 HTTP and WIT: one mapping

Without a written mapping, the runtime, the CLI and the generated clients each
invent their own. There is one, and it is total:

| WIT | Query (`GET`, `HEAD`) | Body (`POST`, `PUT`, `PATCH`) |
| --- | --- | --- |
| `string`, `u8..u64`, `f32`, `f64` | one key per parameter | object field |
| `bool` | `true` / `false` | boolean |
| `enum` | the variant name, kebab-case | string |
| `list<T>` | repeated key | array |
| `option<T>` | absent means `none` | absent or `null` |
| `record` | not allowed in a query | the object itself when it is the only parameter |
| `bytes` | not allowed | raw body when it is the only parameter |

Rules that remove ambiguity:

- A missing required parameter is `invalid` (400); a missing optional one is
  `none`.
- Multiple parameters in a body become an object keyed by parameter name. There is
  no positional form.
- `result<T, error>` maps to `200` with `T`, or to the codes of §7.11.
- `option<T>` in a reply is JSON `null`.
- A body on `GET`, `HEAD` or `DELETE` is `400`.
- An argument that fails its WIT type is `invalid`, never `internal`.

The mapping is implemented once, in the router, and the generated CLI and clients
are produced from the same rule. That is the only way the three agree.

### 7.14 The UI module contract

A slot contribution is an ES module with one required export:

```ts
export function mount(root: HTMLElement, ctx: SlotContext): () => void;
```

- `root` is an empty element owned by the panel.
- `ctx` carries `slot`, `app` (id and display name) and `api`, a client already
  scoped to that app's routes — `/api/apps/<id>/...`, never an absolute URL.
- The returned function unmounts: the panel calls it before replacing the
  contribution and the module must release listeners and timers.
- If `mount` throws, the panel renders a placeholder for that slot and keeps the
  rest of the interface alive.

The module runs inside a sandboxed iframe, but it must not assume it: `ctx.api`
returns relative URLs, so the same module works behind a panel that proxies the
API (§15).

---
