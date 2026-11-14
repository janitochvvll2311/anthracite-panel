# Apps

An app is a signed bundle: a manifest, a WIT package, and either a WebAssembly
component, a native service, or both. The engine verifies the bundle before it
becomes visible and never runs a component that disagrees with its manifest.

## Bundle layout

```
manifest.toml            the index: app facts, hashes, interfaces, routes, slots, CLI
manifest.sig             detached Ed25519 signature over manifest.toml + wit/**
wit/                     the app's WIT package: interfaces and exactly one world
wasm/<name>.wasm         the component (v2/v3 apps with a wasm half)
bin/<name>               the native service (v3 apps with a service half)
web/<module>.js          optional UI modules, hash-pinned by the manifest
```

`wit/` contains the app's package and nothing else. A source tree may keep
`wit/deps/` for the build, but the bundle does not: the engine injects its
catalog and the installed providers when it parses the package. That is what
keeps the signed artifact closed and the interface graph unambiguous.

The signature covers `manifest.toml` and every file under `wit/`, not the wasm,
bin or web bytes directly; those are covered by their hashes in the signed
manifest. `manifest.sig` is the 64-byte Ed25519 signature in standard base64,
one line ([Signing](signing.md)).

## Manifest

`manifest_version = 2` is the original app format; `manifest_version = 3` adds
the `[service]` section and the service/hybrid shapes. v2 bundles stay valid and
unchanged.

| Section | Fields |
| --- | --- |
| `[app]` | `id` (reverse-DNS), `alias` (optional CLI name), `namespace` (`ns:package`), `name`, `version`, `summary`, `license`, `icon`, `icon-sha256` |
| `[requires]` | optional `engine` version |
| `[wasm]` | `path`, `sha256` |
| `[service]` | v3 only: `path`, `sha256`, `class`, `argv` |
| `[[export]]` | `interface`, optional `target = "wasm" \| "service"` |
| `[[import]]` | `interface` |
| `[[route]]` | `method`, `path`, `call` (`interface#function`), `stream`, `transport`, `events`, `write` |
| `[[slot]]` | `id`, `label`, `icon`, `icon-sha256`, `order`, `hidden`, `parent`, `tab`, `view`, `tag`, `module`, `sha256`, `[[slot.actions]]` |
| `[[cli]]` | `call`, `verb` |
| `[limits]` | `fuel`, `memory-pages`, `wall-ms`, `bytes-total`, `bytes-per-sec` |

Unknown keys are errors, not warnings. Two examples:

```toml
# A wasm app (apps/hello)
manifest_version = 2

[app]
id = "org.anthracite.hello"
alias = "hello"
namespace = "anthracite:hello"
name = "Hello"
version = "0.1.0"

[wasm]
path = "wasm/app.wasm"
sha256 = "…"

[[export]]
interface = "anthracite:hello/greeting@1.0.0"

[[import]]
interface = "anth:env/time@1.0.0"

[[route]]
method = "GET"
path = "/hello"
call = "anthracite:hello/greeting@1.0.0#greet"

[[slot]]
id = "panel.view"
label = "Hello"
order = 1
tag = "hello"
module = "web/hello.js"
sha256 = "…"

[[cli]]
call = "anthracite:hello/greeting@1.0.0#greet"
verb = "greet"
```

```toml
# A service app (apps/fs)
manifest_version = 3

[app]
id = "org.anthracite.fs"
namespace = "anthracite:fs"
name = "Files"
version = "0.5.0"

[service]
path = "bin/fs"
sha256 = "…"
class = "host"
argv = ["--socket", "{socket}", "--root", "/", "--trash", "{data}/trash"]

[[export]]
interface = "anthracite:fs/read@1.2.0"

[[route]]
method = "GET"
path = "/files"
call = "anthracite:fs/read@1.2.0#list"
```

### Identity

Interfaces are `namespace:package/interface@semver`. `anth` is the engine's and
the manifest refuses it; `anthracite` is first-party and a **keyed** build refuses
it from a `third-party` bundle ([Signing](signing.md#namespaces)). Anything else
is a third-party package. App ids stay reverse-DNS because that is what ids are
for; the WIT namespace is what the interface graph binds.

Every export must live under the app's own package (`[app] namespace`). Imports
name interfaces the app expects to be provided by the engine or by another
composed app; the resolver checks them before anything is linked.

### Routes and CLI

A `[[route]]` maps one HTTP method and path to an interface function. The engine
only dispatches to a route whose app is running and approved, and the call is
audited. `GET`, `HEAD` and `DELETE` take query parameters; other methods take a
JSON object keyed by the WIT parameters. A `[[cli]]` entry names the same
function; `anth app <alias> <verb>` resolves the alias and calls the route, so a
verb without a route is not shippable.

`stream = true` marks a streaming function; `transport = "websocket"` marks an
upgrade; `events = true` marks a typed-event stream; `write` names the companion
function for a streaming input. The mappings are in [HTTP API](http-api.md#app-routes).

### Slots

A `[[slot]]` contributes a panel module. `id` is a panel-defined slot
(`panel.view`, `panel.action`, `panel.settings`, `panel.fab`, `panel.modal` are
the portable ones), `module` is a path under `web/`, and `sha256` pins its bytes.
The panel fetches the module, verifies the hash and mounts it in a sandboxed
frame ([Panels](panels.md)). `order`, `tag`, `hidden`, `parent`, `tab`, `view`
and declarative FAB `actions` shape the layout; the engine only stores and
serves them.

## Verification

Before an app becomes visible, the engine:

1. unpacks the zip with path safety and size caps;
2. verifies the wasm and service hashes against the manifest;
3. verifies `manifest.sig` when present (distribution key first, then the
   keyring) and derives the tier from the signer;
4. parses the WIT package and checks the world: exactly one `app` world, every
   export under the app namespace, imports resolvable against the catalog and the
   installed providers;
5. computes the risk over the resolved import graph and the service class.

A failure is a typed error: `invalid` for a malformed bundle, `denied` for a
signature, namespace or policy refusal, `too-large` for the caps. An app that
lies in its manifest does not run.

## Install, approve, compose

These are three separate acts:

1. **Install** copies the verified bundle to `$state/apps/<id>`. It needs no
   grant and does not start anything.
2. **Approve** freezes the grant: app id, version, manifest hash, signature, WIT
   set hash, every exported and imported interface, the computed risk, the levels
   the operator accepted, who approved and when. The risk levels are `read`
   (baseline), `write`, `admin`, `host` and `package`; each above `read` needs its
   own `--accept-*` flag. A service's class adds risk (`socket`/`device`/`admin` →
   admin, `exec`/`host` → host).
3. **Compose** names the approved apps and starts them on the next boot/restart.

`anth app approve` with missing levels fails and names the level; the approval
preview (`GET /apps/{id}/approval`, the panel's settings) shows the risk, the
required levels and the interfaces. Installing a service as a third party also
needs `[services] allow-third-party = true` at daemon start; a `system` service
does not.

### Upgrades

`PUT /apps/{id}` (or `anth app upgrade`) replaces the bundle and **revokes the
grant**: the new bytes are not the bytes the operator approved. The panel shows
the authority diff; re-approval is explicit. An upgrade changes the app's
version, interfaces and hashes, so a stale grant is `grant-stale`, not a silent
run.

### Removal

`DELETE /apps/{id}` uninstalls the bundle. If a composed app depends on it, the
answer is `409` with `detail.dependents` and an acknowledgment token; send
`X-Anth-Acknowledge: dependents` to proceed. Removal never deletes the app's
`{data}` or the files it published under `{shared}`; that is an explicit
operator cleanup. The next boot reports the missing provider as a degraded
consumer, not a fatal.

## Limits

Per call: `fuel` (instructions), `memory-pages` (64 KiB pages), `wall-ms` (wall
clock), `bytes-total` and `bytes-per-sec` for streams. The manifest can set them;
the engine's `[limits]` supplies defaults. A hard budget ends with `exhausted`.

## The conformance kit

`anth-conformance` runs the engine's verifier offline. A third party runs it
before publishing:

```sh
anth-conformance [--require-signature] [--allow-third-party-service] \
  [--known <ns:pkg>] [--key <hex-public-key>] [--max-bytes <n>] [--json] bundle.zip
```

It prints the id, version, package, signer, tier, every export and import, and
the service class when there is one. A bundle that lies — a wrong hash, a
declared export the component does not implement, a signature that does not
verify — fails loudly. The same kit is what the release and CI run.

## Building an app

The documented flow, with the SDK:

```sh
# 1. A cdylib for wasm32-unknown-unknown, using anth-guest's wit_bindgen.
cargo build --manifest-path apps/Cargo.toml --release --target wasm32-unknown-unknown

# 2. Wrap the module as a component with the app's world.
wasm-tools component embed --world app apps/<app>/wit target/wasm32-unknown-unknown/release/<crate>.wasm -o embedded.wasm
wasm-tools component new embedded.wasm -o apps/<app>/wasm/app.wasm

# 3. Put the component hash in manifest.toml, then package and sign.
scripts/package-app.sh apps/<app> /tmp/<app>.anth
ANTHRACITE_SIGNING_KEY=release.key scripts/package-app.sh apps/<app> /tmp/<app>.anth

# 4. Check it the way the engine will.
anth-conformance --require-signature --key <public> /tmp/<app>.anth
```

`scripts/build-app.sh` wraps steps 1–2 for the apps in this repository. A service
app builds a native binary instead and lists it under `[service]`
([Services](services.md)). Generated clients for an app's routes come from
`bun scripts/codegen.ts apps/<app> --check` (Rust `anth-client` and the
TypeScript client); the generated surface follows the same WIT.
