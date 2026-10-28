# Plugins

Phase 7 starts with the package layer: signed bundles, frozen grants and a
manager over CLI, HTTP and the **Settings → Plugins** panel. The wasm runtime,
HTTP anchors, iframe bridge and the `hello`/WireGuard reference bundles are the
next cuts; this document marks what is already enforced and what is still
pending.

A plugin extends the panel without receiving authority over the host. The
manifest is a recipe: it declares which programs a plugin needs and the shape of
their arguments. The grant is the policy the operator approved. The guest never
sends an argv: it names a capability and supplies values, and the host renders
the vector from the frozen template.

## Bundle format

```
plugin.toml        manifest
plugin.wasm        guest module (ABI 3)
plugin.sig         detached Ed25519 signature over plugin.toml (optional
                   when require_signature = false)
web/<module>.js    declared UI modules, each with a sha256
web/<module>.css
```

- `plugin.sig` signs the raw bytes of `plugin.toml`, hex encoded (128
  characters).
- The manifest declares the sha256 of the module and of every UI module, so one
  signature approves every byte the host will load or serve.
- `manifest_version = 1` and `abi = 3` are explicit; an unknown value is refused.
- `plugin.wasm` is pinned; the manifest cannot point the module somewhere else.
- UI modules live under `web/`, end in `.js`, `.mjs` or `.css`, and must appear
  in a `[[ui]]` slot with their digest.

Example:

```toml
manifest_version = 1

[plugin]
id = "org.anth.hello"
alias = "hello"
version = "0.3.0"
name = "Hello"
abi = 3

[wasm]
path = "plugin.wasm"
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

[[api]]
method = "GET"
path = "/status"

[[ui]]
slot = "settings.plugin"
tag = "anth-hello"
module = "web/hello.js"
sha256 = "<64 hex characters>"
```

Programs are limited to the host allowlist: `hostname`, `uname`, `id` and `wg`.
Capability ids and UI slots are lowercase dotted names. Argument placeholders
must reference an accepted argument, every accepted argument must be used, and
literal argv elements cannot contain control characters.

## Configuration

```toml
[plugins]
enabled = false
root = "/var/lib/anth/plugins"
install_from_web = false
require_signature = true
public_key = ""
fuel = 50000000
```

- `enabled` is the master switch: install and load refuse when it is off.
- `root` holds one directory per installed plugin.
- `install_from_web` enables `POST /api/plugins/install`. Local CLI install does
  not need it.
- `require_signature` defaults to true. With it off, an unsigned bundle installs
  with a warning; a signature that *is* present is still verified.
- `public_key` is the hex Ed25519 public key. A signed bundle is refused when it
  is empty.
- `fuel` is reserved for the wasm runtime cut.

Grants live beside the config file under `plugins.d/<id>.toml`, mode `0600`.
They are written atomically and are never read from a symlink.

## Install order

Nothing appears under the plugin's id until every step passes:

1. Open the zip and reject path escapes, symlinks, duplicate names, unexpected
   members, more than 512 entries or more than 32 MiB expanded.
2. Read `plugin.toml`, hash it and verify `plugin.sig` when present.
3. Parse and validate the manifest.
4. Hash `plugin.wasm` and every declared UI module.
5. Compare the capability, anchor and UI surface with an existing grant. A
   changed surface is `409 plugin_capabilities_changed` until
   `plugin approve --accept-capabilities` (or the install's
   `accept_capabilities`) rewrites the grant.
6. Rename the staging directory into `plugins/<id>` and write the grant.

A refusal removes staging. At every listing, the host re-verifies the manifest
hash, the signature, the module digest and every UI digest against the grant; a
rewritten bundle becomes an `error` entry with the reason, never a silent skip.

## Grant

```toml
version = 1
id = "org.anth.hello"
name = "Hello"
plugin_version = "0.3.0"
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
slot = "settings.plugin"
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
| `GET /api/plugins` | — | returns the catalogue, `enabled`, `install_from_web` |
| `POST /api/plugins/install` | `{"bundle_base64":"...","accept_capabilities":false}` | `install_from_web` |
| `POST /api/plugins/{id}/enable` | `{}` | — |
| `POST /api/plugins/{id}/disable` | `{}` | — |
| `POST /api/plugins/{id}/remove` | `{}` | — |
| `POST /api/plugins/{id}/approve` | `{"accept_capabilities":true}` | — |

The install answer carries `id`, `name`, `version`, `capabilities` and an
optional `warning` when the bundle was unsigned. Errors use the common shape:
`403 forbidden`, `404 not_found`, `409 conflict` with code
`plugin_capabilities_changed`, or `400 invalid`.

## CLI

```bash
anth-panel plugin list
anth-panel plugin ls
anth-panel plugin install plugins/hello/dist/org.anth.hello.zip
anth-panel plugin install bundle.zip --accept-capabilities
anth-panel plugin disable org.anth.hello
anth-panel plugin enable hello
anth-panel plugin approve org.anth.hello --accept-capabilities
anth-panel plugin remove org.anth.hello --yes
```

Plugin management is local, like `config`: it reads and writes the bundle root
and grant files directly. It does not need a running daemon. The daemon reads
the bundle root and grants on every listing, so installs and toggles apply
without a restart. An alias resolves to its plugin only when it is unique;
otherwise the full id is required.

## Web UI

**Settings** now has **General** and **Plugins** tabs. The Plugins panel follows
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

## Not yet in this cut

- `wasmi` runtime, ABI 3 exports, `anth_exec`/`anth_log` and job records.
- `GET|POST /api/plugins/{id}/{*path}` anchors.
- Declared UI module delivery and the sandboxed iframe/postMessage bridge.
- The `hello` and WireGuard reference bundles and their packaging scripts.

Those pieces build on `PluginService::load` and the frozen grant; the authority
model and file formats above are already enforced.
