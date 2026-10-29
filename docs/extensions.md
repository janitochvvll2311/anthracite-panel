# Extensions

Phase 7 starts with the package layer: signed bundles, frozen grants and a
manager over CLI, HTTP and the **Settings → Extensions** panel. The wasm runtime,
HTTP anchors, iframe bridge and the `hello`/WireGuard reference bundles are the
next cuts; this document marks what is already enforced and what is still
pending.

An extension extends the panel without receiving authority over the host. The
manifest is a recipe: it declares which programs an extension needs and the shape of
their arguments. The grant is the policy the operator approved. The guest never
sends an argv: it names a capability and supplies values, and the host renders
the vector from the frozen template.

## Bundle format

A bundle can keep the whole manifest in the root file or split it by layer:

```text
manifest.toml          root: extension + runtime
manifest.sig           Ed25519 over the manifest set
wasm/manifest.toml     wasm artifact + capabilities
wasm/extension.wasm    guest module (ABI 3)
web/manifest.toml      API anchors + UI slots
web/<module>.js|css    declared UI assets
service/manifest.toml  service socket (service runtime)
```

- The root `manifest.toml` is required and owns `manifest_version`, `[extension]`
  and `[runtime]`.
- The layer files are optional. A single root file may contain all sections.
- A layer manifest writes paths relative to its own directory. The host rewrites
  them to bundle-relative paths once.
- `manifest.sig` signs the ordered manifest set, not only the root file. The
  message includes each file name, length and bytes.
- The manifest declares the sha256 of the wasm module and of every UI module.
- `manifest_version = 2` and `abi = 3` are explicit; an unknown value is refused.
- `wasm/extension.wasm` is pinned; the manifest cannot point the module somewhere
  else.
- UI modules live under `web/`, end in `.js`, `.mjs` or `.css`, and must appear
  in a `[[ui]]` slot with their digest.

### Split example

```toml
# manifest.toml
manifest_version = 2

[extension]
id = "org.anth.hello"
alias = "hello"
version = "0.4.0"
name = "Hello"

[runtime]
kind = "wasm"
```

```toml
# wasm/manifest.toml
[wasm]
path = "extension.wasm"   # resolves to wasm/extension.wasm
abi = 3
sha256 = "<64 hex characters>"

[[capability]]
id = "host.hostname"
program = "hostname"
args = ["--short"]

[[capability]]
id = "host.named"
program = "hostname"
args = ["--alias", "{name}"]
accept = [{ name = "name", type = "string" }]
```

```toml
# web/manifest.toml
[[api]]
method = "GET"
path = "/status"

[[ui]]
slot = "settings.extension"
tag = "anth-hello"
module = "hello.js"       # resolves to web/hello.js
sha256 = "<64 hex characters>"
```

For a service extension, `service/manifest.toml` carries:

```toml
[service]
socket = "/run/user/1000/anth-podman.sock"
```


Programs are limited to the host allowlist: `hostname`, `uname`, `id` and `wg`.
Capability ids and UI slots are lowercase dotted names. Argument placeholders
must reference an accepted argument, every accepted argument must be used, and
literal argv elements cannot contain control characters.

## Configuration

```toml
[extensions]
enabled = false
root = "/var/lib/anth/extensions"
install_from_web = false
require_signature = true
public_key = ""
fuel = 50000000
```

- `enabled` is the master switch: install and load refuse when it is off.
- `root` holds one directory per installed extension.
- `install_from_web` enables `POST /api/extensions/install`. Local CLI install does
  not need it.
- `require_signature` defaults to true. With it off, an unsigned bundle installs
  with a warning; a signature that *is* present is still verified.
- `public_key` is the hex Ed25519 public key. A signed bundle is refused when it
  is empty.
- `fuel` is reserved for the wasm runtime cut.

Grants live beside the config file under `extensions.d/<id>.toml`, mode `0600`.
They are written atomically and are never read from a symlink.

## Install order

Nothing appears under the extension's id until every step passes:

1. Open the zip and reject path escapes, symlinks, duplicate names, unexpected
   members, more than 512 entries or more than 32 MiB expanded.
2. Read the manifest set, hash it and verify `manifest.sig` when present.
3. Parse and validate the manifest.
4. Hash `wasm/extension.wasm` and every declared UI module.
5. Compare the capability, anchor and UI surface with an existing grant. A
   changed surface is `409 extension_capabilities_changed` until
   `extension approve --accept-capabilities` (or the install's
   `accept_capabilities`) rewrites the grant.
6. Rename the staging directory into `extensions/<id>` and write the grant.

A refusal removes staging. At every listing, the host re-verifies the manifest
hash, the signature, the module digest and every UI digest against the grant; a
rewritten bundle becomes an `error` entry with the reason, never a silent skip.

## Grant

```toml
version = 2
id = "org.anth.hello"
name = "Hello"
extension_version = "0.3.0"
runtime = "wasm"
enabled = true
manifest_sha256 = "..."
manifest_signature = "..."
wasm_sha256 = "..."
installed_at_ms = 1

[[capability]]
id = "host.hostname"
program = "hostname"
args = ["--short"]

[[api]]
method = "GET"
path = "/status"

[[ui]]
slot = "settings.extension"
tag = "anth-hello"
module = "web/hello.js"
sha256 = "..."
```

The grant is the authority, not the manifest. A reinstalled bundle that asks for
a new program is refused until the operator approves it; the advertised
capability list comes from the grant.

## HTTP

All routes require a live panel session unless auth is disabled. Mutations check
`Origin` against `Host` when the header is present.

| Route | Body | Policy |
| --- | --- | --- |
| `GET /api/extensions` | — | returns the catalogue, `enabled`, `install_from_web` |
| `POST /api/extensions/install` | `{"bundle_base64":"...","accept_capabilities":false}` | `install_from_web` |
| `POST /api/extensions/{id}/enable` | `{}` | — |
| `POST /api/extensions/{id}/disable` | `{}` | — |
| `POST /api/extensions/{id}/remove` | `{}` | — |
| `POST /api/extensions/{id}/approve` | `{"accept_capabilities":true}` | — |
| `GET|POST|PUT|PATCH|DELETE /api/extensions/{id}/{*path}` | raw body, max 1 MiB | anchor declared in the grant |
| `GET /api/extensions/{id}/web/{*path}` | — | declared UI module |
| `GET /e/{id}/{*module}` | — | host-owned iframe bootstrap |

The install answer carries `id`, `name`, `version`, `capabilities` and an
optional `warning` when the bundle was unsigned. Errors use the common shape:
`403 forbidden`, `404 not_found`, `409 conflict` with code
`extension_capabilities_changed`, or `400 invalid`.

An anchor must match a `[[api]]` method and path in the frozen grant; an
undeclared pair is `404`. The body is passed to the guest as text, not parsed by
the host. A guest reply `{"status":200,"body":...}` becomes that HTTP status;
a string body is `text/plain`, anything else is JSON, and every response is
`nosniff`. Extension log lines go to the daemon log with the extension id, never to
the HTTP response.

UI assets are served only when a `[[ui]]` slot declares that exact module. The
handler canonicalises the path, refuses a symlink that leaves the bundle, caps
the file at 4 MiB, and sends `nosniff`, `no-store` and an explicit content type.

## CLI

```bash
anth-panel extension list
anth-panel extension ls
anth-panel extension install extensions/hello/dist/org.anth.hello.zip
anth-panel extension install bundle.zip --accept-capabilities
anth-panel extension disable org.anth.hello
anth-panel extension enable hello
anth-panel extension approve org.anth.hello --accept-capabilities
anth-panel extension remove org.anth.hello --yes
anth-panel extension call org.anth.hello '{"action":"greeting"}'
printf '{"action":"greeting"}' | anth-panel extension call hello -
```

Extension management is local, like `config`: it reads and writes the bundle root
and grant files directly. It does not need a running daemon. The daemon reads
the bundle root and grants on every listing, so installs and toggles apply
without a restart. An alias resolves to its extension only when it is unique;
otherwise the full id is required.

## Web UI

**Settings** now has **General** and **Extensions** tabs. The Extensions panel follows
the section template:

- One feed, status/count toolbar, refresh, policy strip and a local filter.
- Cards show id, alias, version, state, error reason, capabilities and anchors.
- `Enable`/`Disable`, `Approve` and `Remove` act on one entry; approve and
  remove ask for confirmation.
- Browser install shows a file picker and requires explicit trust before the
  bundle is read; a successful unsigned install surfaces the warning.
- When `install_from_web` is off, the panel points to the CLI instead of showing
  the picker.
- The layout stacks at the component breakpoint and keeps 44 px touch targets.

## Runtime

The guest runs in `wasmi`, with no JIT and no WASI. Each call compiles the
module once, creates a fresh store and invokes one of the three entries:
`anth_http`, `anth_cli` or `anth_direct`. Only `anth_exec` and `anth_log` are
imported; anything else fails to link.

- `[extensions] fuel` (default 50,000,000) bounds instructions.
- Linear memory is capped at 64 MiB; one instance, table and memory.
- One call may run at most 32 capabilities; stdout from a capability is capped
  at 64 KiB and guest log at 64 KiB.
- A trap, an out-of-fuel call or a malformed ABI is a transport error, not a
  extension reply.
- The guest names a capability; the host renders the argv from the frozen
  template and validates every value. A value never becomes two arguments and
  a leading `-` is refused.

`extension call` uses the `anth_cli` entry and prints
`{"status":...,"value":...}`; a status of 400 or above exits non-zero. HTTP
anchors use `anth_http`; the host checks the method and path against the grant
before running the guest.

## UI isolation

A declared UI slot is mounted by `<anth-extension-frame>`:

- The iframe uses `sandbox="allow-scripts"` with no `allow-same-origin`, so the
  module has an opaque origin and no panel DOM, cookies or storage.
- `/e/{id}/{module}` serves a host-owned bootstrap, not the raw module. The
  bootstrap inlines the declared source as a blob import, sends `ready`, and
  waits for the parent's init message.
- The bootstrap CSP is `default-src 'none'`, a per-request nonce for its own
  script, `blob:` for the module, `style-src 'unsafe-inline'`, data/blob images,
  no network (`connect-src 'none'`) and `frame-ancestors 'self'`.
- `postMessage` is accepted only from the frame's own `contentWindow`, with an
  opaque (`null`) origin and the `anthExtension` marker.
- The parent sends the extension/slot snapshot, the panel locale and theme tokens.
- `api.call(path, body, method)` is checked against the extension's frozen anchors
  before the parent performs the same-origin fetch. An undeclared method/path
  is refused without a network request.
- `api.resize(height)` lets the widget size itself; errors show inside the
  widget card and do not break the shell.

The module contract is an exported `mount(root, api)` (or a default function).
It is self-contained: dynamic imports beyond the blob are blocked by the CSP.

## Reference extensions

Two peer projects show the contract end to end. They are not workspace members:
`backend/` does not build them and `frontend/` does not install their
dependencies.

### `extensions/hello`

The smallest complete bundle: one capability (`host.hostname`), two anchors
(`/greeting`, `/status`) and one UI slot. Its widget calls `/greeting` through
the iframe bridge.

```bash
cd extensions/hello
./package.sh
# dist/org.anth.hello.zip, dist/hello-public-key.txt
```

### `extensions/wireguard`

A read-only WireGuard bundle: `wg show interfaces` and `wg show <if>`, with the
interface validated by the host's `ifname` type. The widget lists interfaces and
shows one. Interface/peer mutations, config persistence and private-key handling
are deliberately not in this cut.

```bash
cd extensions/wireguard
./package.sh
# dist/org.anth.wireguard.zip, dist/wireguard-public-key.txt
```

Each script builds the guest for `wasm32-unknown-unknown`, copies the declared
UI module, fills both digests, signs the manifest with Ed25519 and writes the
zip. The key lives under `dist/` and is gitignored; reuse it for upgrades so the
operator's `[extensions] public_key` stays valid. Set that key before installing.

```bash
sudo anth-panel extension install extensions/hello/dist/org.anth.hello.zip
curl http://127.0.0.1:8080/api/extensions/org.anth.hello/greeting
```

The hello widget was exercised in the browser with the real runtime: the
sandboxed iframe called `/greeting`, the host ran `hostname` through the frozen
grant and the widget displayed `Hola desde <host>`. The WireGuard bundle was
installed and listed; the container has no `wg` binary, so its anchors report
the host refusal there.

Phase 7 is code-complete with these two bundles; the WireGuard mutation surface
is a later feature, not part of the extension isolation phase.
