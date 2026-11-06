# 7. The app

> Part of the [design index](../../DESIGN.md).

### 7.1 Bundle layout

```
manifest.toml            # the proposal; the signable index
manifest.sig             # detached signature over the manifest set
wit/                     # the app's WIT package: interfaces and exactly one world
wasm/<name>.wasm         # the component; required when the app exports an interface
web/<name>.js            # optional UI module(s), hash-pinned by the manifest
```

`wit/` holds the package and nothing else. `wit/deps/` is not needed: the engine
resolves `use` against its own host catalog and the packages of installed
providers ([§7.3](07-app.md#73-wit-as-the-contract)), so a vendored copy cannot go stale.

A `bin/` entry joins this layout in v0.6.0 with the first service app ([§7.4](07-app.md#74-app-kinds)).

**How the component is built.** The target is `wasm32-unknown-unknown`, not
`wasm32-wasip2`:

```bash
cargo build --release --target wasm32-unknown-unknown
wasm-tools component embed --world app wit/ target/…/<name>.wasm -o embedded.wasm
wasm-tools component new embedded.wasm -o wasm/<name>.wasm
```

`wasm32-wasip2` also produces a component, but it links the WASI preview 2
interfaces the standard library uses — `wasi:cli`, `wasi:io`, `wasi:clocks` — and
those imports are ambient authority nobody granted. The world check of
[§7.12](07-app.md#712-enforcement-the-world-must-match-the-grant) would refuse
every app built that way, including our own. The core target keeps `std` and
produces a component with exactly the imports the app declared. The SDK wraps
these three commands so an author never types them.

Rules inherited from v0.4.0, kept because they worked: no absolute paths, no
`..`, no symlinks, a bounded entry count, a bounded size per file, every declared
hash verified before the bundle becomes visible.

An app author gets the host interfaces from `sdk/rust/guest`, which packages
`anth:env`, `anth:store/kv` and `anth:error`: depend on the crate and the
`import`s resolve. `engine/wit/` stays the source of truth and a CI check compares
the two, so the packaged copy cannot drift.

### 7.2 Manifest

The manifest is an **index**, not a type definition. Types live in WIT, so the
manifest only says *which* interfaces and *how* they are reached. `[app] namespace`
is the WIT package's name: the validator requires the `package` line under `wit/`
to use the same `namespace:package`. The **package version versions the interface
set** and is not the app's version (D-Interface-Versioning); every exported
interface id carries it, so the package name is written in exactly one place.

```toml
manifest_version = 2

[app]
id = "org.example.notes"
alias = "notes"                    # the CLI name; falls back to the id's last segment
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

# ---- what it needs ----
[[import]]
interface = "anthracite:fs/list@1.0.0"

[[import]]
interface = "anth:store/kv@1.0.0"

# ---- HTTP surface ----
[[route]]
method = "GET"
path   = "/notes"
call   = "example:notes/list@1.0.0#all"

# ---- UI surface ----
[[slot]]
id     = "panel.view"
label  = "Notes"
icon   = "notes"
order  = 10
tag    = "anth-notes"
module = "web/notes.js"
sha256 = "…"

# ---- CLI surface ----
[[cli]]
call = "example:notes/list@1.0.0#all"
verb = "list"

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
package anthracite:fs@1.0.0;

interface list {
  use anth:error/kinds@1.0.0.{error};

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
| `anth` | the engine | `anth:env/info@1.0.0` |
| `anthracite` | first-party apps | `anthracite:hello/greeting@1.0.0` |
| anything else | third-party apps | `example:notes/list@1.0.0` |

An app cannot claim `anth` or `anthracite` unless its tier matches. App ids stay
in reverse-DNS (`org.example.notes`) because that is what DNS-shaped ids are for,
while the app's WIT namespace is declared in the manifest and must be unique
across a composition.

**Two version numbers, two meanings.** The package version versions the interface
set (`anthracite:hello@1.0.0`); the app has its own version
(`anthracite:hello@0.5.1` in the manifest). The interface id carries the package
version, so an app may ship 0.5.1 with an unchanged `greeting@1.0.0` and consumers
do not notice. WIT has no per-interface version: changing any interface in the
package bumps the package, and therefore every id in it.

**Versioning is exact.** The component `Linker` resolves imports by name, so
`@1.0.0` and `@1.0.1` are different interfaces and do not link. This is not a
policy choice, it is what the tool does. To evolve an interface, a provider
exports the old and the new version side by side and consumers migrate on their
own schedule. No ranges, no negotiation, no silent widening.

**Files, not strings.** WIT lives in `wit/` inside the bundle, covered by the
signature. It is a language with tooling: embedding it in TOML would make it
unformattable and unreviewable.

**One package, one world.** `wit/` holds exactly one package, and that package
declares exactly one world — the one the component implements. Zero or two worlds
is an install error, not a convention to infer.

**Resolution at install time.** The package's `use`s are resolved against the
engine's host catalog plus the packages of installed providers. Nothing is
vendored: a copy of `anth:error` inside a bundle would be a second truth that
drifts, and validating against what is installed is stronger than validating
against what the author shipped.

**Two sources, one truth.** The manifest declares imports and exports so the
composer can resolve *before* instantiating. The component embeds the real world.
The engine compares them and refuses the app on any difference ([§7.12](07-app.md#712-enforcement-the-world-must-match-the-grant)). The
manifest lets us plan; the component is what we check.

**What the signature covers.** One detached signature over a canonical message:

```
"anth-manifest-v2\0"
for each file in sorted order — `manifest.toml` and every file under `wit/`:
    name, "\0", length as little-endian u64, bytes
```

The wasm and the UI modules are not in the message; they are covered by their
hashes inside the manifest, and those hashes are covered by the signature. One
rule, written once, checked by one test with a fixed vector.

Names are plain relative paths: `[A-Za-z0-9._/-]` only. The NUL is the field
separator, so a name containing one (or any other control byte) would let two
different sets sign the same bytes; a bundle with such a path is refused.

### 7.4 App kinds

One app may combine three implementations; the contract is the same for all of
them.

| Kind | Runs as | May request | Typical use |
| --- | --- | --- | --- |
| `wasm` | in-process component, sandboxed, fuel-metered | any granted interface | all logic, including heavy compute |
| `ui` | sandboxed iframe served by the engine | RPC to its own app only | rendering |

**`service` arrives in v0.6.0, with the terminal.** A service is a supervised child
process, and it is the one kind that cannot speak WIT: it is a binary, and the only
way to reach it is a socket. That is an exception to [§3.5](appendix-a-vertical-slice.md), and an exception needs
its first real consumer — a PTY, with half-close and out-of-band control — to be
designed correctly. Until then the tier rule already stands: **a third-party app
never gets a service.**

An app may be wasm-only (a provider such as `fs`), ui-only (a slot contribution
with no logic), or both. `[wasm]` is required if and only if the app declares at
least one `[[export]]`; the validator says so in the error rather than letting an
author ship an empty component.

### 7.5 The component world

An app's component has one world. A minimal one:

```wit
package example:notes@0.1.0;

world app {
  import anthracite:fs/list@1.0.0;
  import anth:store/kv@1.0.0;

  export list;
}
```

That is the entire ABI. There is no `anth_call`, no packed pointers, no JSON
envelope for host calls: values are lifted and lowered by the canonical ABI, and
every host import is an interface the broker implements.

Cross-app calls are component links: the composer satisfies `anthracite:fs/list@1.0.0`
with the instance of the app that exports it. No serialization, no router, no
broker.

### 7.6 Routes

A route is `METHOD /path -> interface#function`. The engine mounts them under
`/api/apps/<app-id>/`, so two apps may both declare `/status` without colliding
and an operator can tell from the URL whose code is answering. The accepted verbs
and the exactness of the match are fixed in [§6.4](06-engine.md#64-http-surface).

**Path rules, checked at install.** A route path starts with `/`, has no empty
segment, and contains no `..`. Under `/api/apps/<id>/`, `assets` is reserved for UI
modules: a route whose path is `assets` or starts with `assets/` is refused, so an
app cannot shadow the delivery of its own UI.

```
GET /api/apps/org.example.notes/notes
```

The engine maps the HTTP request to function arguments with the single rule of
[§7.13](07-app.md#713-http-and-wit-one-mapping), and the reply to the status codes of [§7.11](07-app.md#711-errors). A guest trap maps to `internal`
with a distinct code, sanitized at the boundary.

Argument and reply values are type-checked at the canonical ABI boundary by
construction; there is no second validation pass to forget.

### 7.7 UI slots

A slot is a named mount point in a panel's layout. Three parties know different
things: the panel knows where its holes are, the app knows where it wants to
appear, and **the engine knows neither** — it stores the contribution and serves
the asset, nothing else.

The **SDK defines the vocabulary**; a panel declares which slots it implements.
`panel.` is the prefix of the contract, not a brand: a third-party panel that
implements `panel.view` is implementing the contract, not borrowing our name.

| Slot | The app provides | The panel does |
| --- | --- | --- |
| `panel.view` | `label`, `icon`, `order`, `tag`, `module`, `sha256` | derives the sidebar entry, opens the view as a tab, and may detach it to a floating window |
| `panel.action` | `label`, `icon`, `order`, and a `target` — its own view or a `METHOD /path` of the same app | lists it in the action button |
| `panel.settings` | `label`, `order`, `tag`, `module`, `sha256` | renders it as a section inside settings |

A panel may declare slots of its own, but those are not portable: an app targeting
them will not appear anywhere else.

`order` is an integer, default `0`, ties broken by app id, so a contribution's
position is deterministic instead of depending on map iteration.

The engine validates only the **format** of a slot id (`[a-z][a-z0-9.-]*`, bounded
length). It cannot validate membership, because it does not know the panel — and it
must not learn. A contribution to a slot the panel does not implement is ignored;
the panel reports it in development and the composition view lists it, so an
operator can see it exists.

Modules are served with `ETag` set to their pinned hash and `Cache-Control:
immutable` ([§6.4](06-engine.md#64-http-surface)), so a panel reload is a `304`, not a download.

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
package anthracite:fs@1.0.0;

interface changed {
  record event { path: string, kind: enum { created, modified, removed } }
  on-change: func(event: event);
}
```

Consumers import it, the publisher emits it, the engine fans out. No strings, no
payload schemas, no subscription grants.

### 7.9 CLI and clients

WIT gives the shape; the manifest chooses the surface. Every command is a
`[[cli]]` entry naming an `interface#function`; anything else would make the CLI a
generic HTTP client holding an operator's credentials.

**Invocation.** `anth app <alias> <verb>`:

- `alias` comes from the descriptor's **operator alias**, then `[app] alias`,
  falling back to the last segment of the id. An operator alias may not collide
  with another installed app's effective alias; two fallbacks may collide, and
  then the CLI requires the full id instead of guessing.
- `verb` is one token, unique within the app.
- `list`, `show`, `install`, `upgrade`, `remove`, `approve`, `revoke`, `enable` and
  `disable` are reserved: an **alias** may not use them, because `anth app list`
  must mean one thing. A verb never collides, since it is always the second token
  (`anth app notes list`).
- The full id is always accepted: `anth app org.example.notes list`.

**Where the surface comes from.** `GET /api/engine/composition` carries
`cli: [{verb, interface, function}]` per app, so the CLI never parses a manifest.
A consequence worth stating: **only composed apps have verbs**. An installed but
uncomposed app can be managed, not invoked — it has no instance to answer.

**Flags** are the inverse of [§7.13](07-app.md#713-http-and-wit-one-mapping): one flag per parameter, named after it,
optional for `option<T>`, repeated for `list<T>`, and `--flag` / `--no-flag` for
`bool`.

**Output.** The engine always answers JSON; the CLI renders it. Three mutually
exclusive views, with `--table` as the default:

| Flag | Use |
| --- | --- |
| `--table` | human reading; columns inferred from the value |
| `--json` | scripts and tests; the only format golden tests use |
| `--yaml` | human reading of nested values |

The table is inferred from the value, not from WIT types: a list of records becomes
columns from its keys, a record becomes a key/value table, a scalar a single line,
and an empty list has no columns and prints `(no rows)`. Width never changes the
content, so the same command yields the same cells on a terminal and in a pipe.

**Exit codes** follow [§7.11](07-app.md#711-errors) and are documented once, so a script can tell "does not
exist" from "not allowed" without parsing text.

**Clients.** The same generator emits the Rust client and the TypeScript client
from the same WIT. Hand-written CLI and hand-written clients drift from the
server; v0.4.0 proved it, and the fix is generation, not discipline.

**The engine commands** are fixed, not generated:

| Command | HTTP | Purpose |
| --- | --- | --- |
| `anth status` | `GET /api/health` + `/api/engine/composition` | summary, degraded apps and why |
| `anth doctor` | — (offline) | validate config, composition and grants with no daemon |
| `anth compose [--show\|--add\|--remove\|--now]` | `GET`/`PUT /api/engine/composition` | read and edit the composition |
| `anth restart` | `POST /api/engine/restart` | restart the service |
| `anth login` / `anth logout` | `POST`/`DELETE /api/engine/session` | session over TCP |
| `anth app list\|show\|install\|upgrade\|approve\|revoke\|enable\|disable\|remove` | the app resources of [§6.4](06-engine.md#64-http-surface) | lifecycle |
| `anth key add\|list\|remove` | `/api/engine/keys` | the third-party keyring |
| `anth audit` | `GET /api/engine/audit` | the log |

`doctor` answers "would this boot?" without a daemon; `status` answers "is it
healthy now?" with one. The first is what saves you when the engine does not
start.

**Breaking actions ask for acknowledgment.** Disabling, removing or revoking an app
that other composed apps depend on returns `409` with
`detail.dependents` and `detail.acknowledge`. The CLI prints the warning and asks;
`--acknowledge <token>` skips the question:

```
$ anth app disable org.anth.fs
warning: org.anth.fs is required by org.example.notes, org.anth.report
rerun with --acknowledge dependents to proceed
$ anth app disable org.anth.fs --acknowledge dependents
```

The token is never guessed: it comes from `detail.acknowledge`. Without a TTY and
without the flag, the command fails with that message instead of waiting for an
answer that will never come.

The pattern is general: **any action that breaks consumers, revokes authority or
cannot be undone returns the tokens it needs, and the engine audits the
acknowledgment with the names of what was broken.**

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

It is a host interface ([§9.2](09-authority.md#92-the-closed-native-capability-set)), so it is audited and quota'd by the broker.

Blobs are **not** in v0.5.0. They arrive with transfers in v0.6.0 as
`stream<u8>`, because a `bytes` cap cannot carry a large file and switching the
contract later would break the first app that used it. The native set is what the
resolver treats as always available, so declaring a capability that the broker
does not implement would turn a manifest error into a runtime `503`.

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

Every error, from an app or from the engine, uses one envelope:

```json
{"error":{"code":"unavailable","message":"…","app":"org.example.notes"}}
```

| Field | Required | Notes |
| --- | --- | --- |
| `code` | yes | the discriminant; stable |
| `message` | yes | sanitized at the HTTP boundary |
| `app` | no | the app involved, when there is one |
| `detail` | no | structured extras, keyed by code |

The mapping from the variant is total: every case produces a code, and
`too-large(u64)` also produces `detail.limit`.

| Code | HTTP | Exit | Produced by |
| --- | --- | --- | --- |
| `denied` | 403 | 3 | an app refusing, or a capability not granted |
| `unavailable` | 503 | 5 | provider missing or degraded |
| `invalid` | 400 | 2 | arguments refused |
| `not-found` | 404 | 4 | unknown resource or route |
| `too-large` | 413 | 6 | payload over the limit; `detail.limit` |
| `internal` | 500 | 1 | app bug or trap; sanitized |
| `unauthenticated` | 401 | 3 | no session, or an expired one |
| `conflict` | 409 | 2 | id exists, duplicate provider, broken dependents |
| `precondition-failed` | 412 | 2 | `If-Match` does not match |
| `method-not-allowed` | 405 | 2 | the verb is not declared |
| `exhausted` | 503 | 5 | the call exceeded its fuel, memory or wall budget; `detail.budget` |

The first six are the ones an app can produce, because they are the variant. The
last five belong to the engine's own API.

A budget trap is `exhausted`, not `internal`: the app did not crash, it ran out of
the time or memory the operator granted it. The distinction matters to whoever
reads the log, and to the client deciding whether to retry.

**Forward compatibility.** A client that does not know a `code` treats it as
`internal`. Adding a code is therefore not a breaking change, and the SDK does not
need a closed enum.

`detail` carries what a client needs in order to act. Two shapes exist today:
`too-large` → `{"limit":n}`, and `conflict` on a breaking operation →
`{"dependents":[…],"acknowledge":["dependents"]}` ([§6.4](06-engine.md#64-http-surface)).

Exit `0` means success. The mapping is defined once, so a script can branch on the
kind instead of parsing a message.

`internal` messages are scrubbed at the HTTP boundary: a third-party app never
gets to leak host paths or engine internals to a browser.

### 7.12 Enforcement: the world must match the grant

This is the security core, and it happens before a single instruction runs:

1. Parse the package under `wit/`, with the host catalog and the installed
   providers injected, and build the expected world.
2. Read the component's real type with `Component::component_type()`.
3. Compare imports and exports: names, versions and structural types. Any
   difference, including an extra import, is a refusal.

Each interface has a **world signature**: lowercase hex SHA-256 of its canonical
WIT text (`interface <name> { … }`, declarations sorted, no whitespace). The
expected world carries one for every interface; the component must match it, and
a missing actual signature is drift, not a pass. `anth:error` is injected when the
WIT is parsed: an **import** of it is never granted or compared (§9.2), but an
export claiming that namespace is still a real export and is refused like any
other unapproved one.
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
| `string`, `u8..u64`, `s8..s64`, `f32`, `f64` | one key per parameter | object field |
| `bool` | `true` / `false` | boolean |
| `enum` | the variant name, kebab-case | string |
| `list<T>` | repeated key | array |
| `option<T>` | absent means `none` | absent or `null` |
| `record` | not allowed in a query | the object itself when it is the only parameter |
| `bytes` | not allowed | raw body when it is the only parameter |

Only the types in that table are allowed in a route signature. `tuple`, `flags`,
`char` and a nested `result` are refused at install time with a clear error,
rather than mapped by guesswork.

Rules that remove ambiguity:

- A missing required parameter is `invalid` (400); a missing optional one is
  `none`.
- A repeated query key is a list; `key[]=value` expresses a single-item list.
- `OPTIONS` on an app route is answered by the engine with `204` and `Allow`;
  a method the manifest does not declare is `405` with the same `Allow` list.
- Multiple parameters in a body become an object keyed by parameter name. There is
  no positional form.
- `result<T, error>` maps to `200` with `T`, or to the codes of [§7.11](07-app.md#711-errors).
- `option<T>` in a reply is JSON `null`.
- A body on `GET`, `HEAD` or `DELETE` is `400`.
- An argument that fails its WIT type is `invalid`, never `internal`.

The mapping is implemented once, in the router, and the generated CLI and clients
are produced from the same rule. That is the only way the three agree.

**What the router needs.** A route names `interface@version#function`. The version
is part of the exported name — a component exposes `example:spike/ping@0.1.0`, not
`example:spike/ping` — so the router resolves the instance first and the function
inside it.

The parameter and result types come from `Component::component_type()` at runtime,
not from a compile-time binding: an installed app is an artifact, so its signature
is read rather than assumed. A function whose types fall outside the table above is
refused at install, with the offending type named.

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
- **The panel owns the lifetime.** A mounted view stays mounted while its tab
  exists: switching tabs hides it instead of unmounting it, so an editor keeps
  half-written state. Modules must tolerate being hidden and shown.
- **A view must be re-mountable.** Detaching to a floating window unmounts from one
  root and mounts into another. Anything global — listeners, timers, state kept in
  the document — has to be released by the cleanup function.
- If `mount` throws, the panel renders a placeholder for that slot and keeps the
  rest of the interface alive.

The module runs inside a sandboxed iframe, but it must not assume it: `ctx.api`
returns relative URLs, so the same module works behind a panel that proxies the
API ([§15](15-open-decisions.md#15-open-decisions)).

---

### 7.15 Streams

Streams and futures are **values**, not a different calling convention:

```wit
follow: func(path: string) -> result<stream<line>, error>;
```

The function is synchronous and returns a handle. The caller decides what to do
with it, and that decision is local:

- consuming is `block_on(reader.read(buf))` inside an ordinary `fn`;
- producing is `start_task` plus a writer, because a stream cannot be returned
  before something exists to fill it.

`async func` in WIT is **not used**. It works — the async ABI is in wasmtime and
in `wit-bindgen` — but it makes asynchrony contagious: one async export forces its
consumers to await, up to the router, and gives the HTTP surface two shapes
instead of one. With streams as values, an app that reads a blocking source still
exports a plain function, and the concurrency stays inside it.

The door stays open: interfaces are versioned, so `anthracite:fs/read@1.1.0` could
be async while `@1.0.0` stays synchronous. Adopting it is a decision with a real
case behind it, not a default.

**The engine runs calls concurrently anyway.** A guest blocked on a stream must not
block the daemon, so the concurrent store and the long-lived instances live in
`anth-runtime` (§6.3). The contract stays simple; the runtime does the work.

**HTTP mapping.** A route whose function returns a stream maps by element type,
because a stream has no other meaning over HTTP:

| Result | Response |
| --- | --- |
| `stream<list<u8>>` | `application/octet-stream`, chunked |
| `stream<record>` | `text/event-stream`, one JSON object per event |
| `future<T>` | `200` with `T` once it resolves |

Cancelling the response cancels the stream: the guest's writer observes
`Dropped`, which is the component model's cancellation signal.
