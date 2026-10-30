# Capability registry

Status: **contract for the v0.4.0 migration**. Phase 0 (schemas), Phase 1
(registry and `app check`) and the grant, risk and approval preview of Phase 2 are implemented.
Manifest v2 and the v2 grants still load for existing bundles, and the v3 path
is live: signed v3 bundles install, approve, enable, disable, remove, serve
anchors and UI assets, and the runtime accepts ABI 4 with
`anth_capability_call` and `org.anth.host.info`. `hello` is migrated; WireGuard
and the Podman helper are pending. This document is normative for the new
manifest (v3), grant (v3), app ABI (4) and registry.

## Model

An app is a **wasm** bundle. Native code is a capability implementation, never a
second app runtime.

```text
wasm app ── capability_call(id, request) ──► panel broker
                                               │
                    ┌──────────────────────────┼─────────────────────────┐
                    ▼                          ▼                         ▼
              host operation             wasm export              binary/service
              (Rust, typed)              (same bundle)            (sandbox + fd)
```

The host is the only enforcement point. The wasm never supplies a path, a
binary or an argv outside what the manifest declared and the operator approved.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Export | A capability the app provides: `id@version`, input/output schemas, implementation. |
| Import | A capability the app consumes from the host: `id@version`, schemas. |
| Registry | The deterministic graph of exports, imports, CLI, API and UI entries. |
| Grant | The operator's frozen approval of the normalized capability set. |
| Provider | `host` in v1; an app in a later phase. |
| Alias | Operator config that maps a host capability to a plugin export. |
| Risk | Host-computed: `read`, `manage`, `admin`, `host`. |

## Manifest v3

One manifest may contain every section. Layer manifests are merged before
validation; paths in a layer are relative to that layer.

```toml
manifest_version = 3

[app]
id = "org.anth.podman"
version = "0.1.0"
name = "Podman"

[runtime]
kind = "wasm"

[wasm]
path = "wasm/plugin.wasm"
sha256 = "<64 hex>"
abi = 4

[[export]]
id = "org.anth.podman.containers.list"
version = 1
reason = "List containers for the Podman section"
input = { type = "record", fields = {} }
output = { type = "record", fields = {
  containers = { type = "array", items = { type = "json" } }
} }

[export.implementation]
kind = "service"
source = "bundle"
path = "helpers/anth-podman"
sha256 = "<64 hex>"
protocol = "http-fd"
idle_seconds = 300
sandbox = "resource"
limits = { memory_bytes = 67108864, pids = 32, wall_seconds = 30 }

[[export.implementation.resource]]
kind = "socket"
path = "/run/user/1000/podman.sock"
as = "podman"

[[cli]]
name = "containers"
export = "org.anth.podman.containers.list"

[[api]]
method = "GET"
path = "/containers"
export = "org.anth.podman.containers.list"

[[ui]]
slot = "settings.app"
tag = "anth-podman"
module = "web/podman.js"
label = "Podman"
sha256 = "<64 hex>"
```

### Exports

- `id` is `id@version`; `version` starts at 1.
- `id` must equal the app id or start with `<app-id>.`.
- `input` and `output` are schemas from the type language below.
- `implementation` is required and must declare its authority:
  - `host`: the panel operation to call.
  - `wasm`: an entry in the app's own module.
  - `binary`: a one-shot native program.
  - `service`: a long-lived helper with a protocol and idle timeout.

A native implementation also declares `sandbox`, `resource` and `limits`.
A `read-only` sandbox cannot open resources.

### Imports

```toml
[[import]]
id = "org.anth.host.info"
version = 1
provider = "host"
input = { type = "record", fields = {} }
output = { type = "record", fields = {
  hostname = { type = "string" },
  os = { type = "record", fields = {
    name = { type = "string" }, version = { type = "string" },
    kernel = { type = "string" }, arch = { type = "string" }
  } },
  uptime_seconds = { type = "int", min = 0 },
  cpu_model = { type = "string" },
  logical_cores = { type = "int", min = 1 },
  load = { type = "record", fields = {
    one = { type = "float" }, five = { type = "float" }, fifteen = { type = "float" }
  } },
  memory = { type = "record", fields = {
    total_kib = { type = "int", min = 0 }, available_kib = { type = "int", min = 0 },
    swap_total_kib = { type = "int", min = 0 }, swap_free_kib = { type = "int", min = 0 }
  } },
  collected_at_ms = { type = "timestamp" }
} }
```

- v1 imports must target `org.anth.host.*` and use `provider = "host"`.
- The signature must match a host export exactly. There is no compatibility or
  auto-binding.
- An import with no exact host export leaves the app `unresolved`.
- Plugin-to-plugin imports are rejected in v1.

## Type language

```text
bool
int(min?, max?)
float
string
string(enum: [...])
string(uuid)
string(path: app|host)
string(id: <domain>)
bytes                       # base64 on the wire
timestamp                   # Unix milliseconds
json
record { name: type }       # closed by default
array(type)
map(type)
union { tag: type }         # discriminated by "type"; the tag is not repeated
nullable(type)
```

Rules:

- Records are closed: unknown fields are rejected.
- Optional fields are explicit: `{ type = "string", optional = true }`.
- `path` and `id` are domains, not free strings. The capability decides what a
  path is rooted at and which ids it accepts.
- Schemas are bounded: depth 16, 64 fields, 32 union variants, 64 enum values,
  64 KiB canonical JSON.
- Values are bounded: 65 536 array/map items, 8 MiB byte strings.

### Canonical signature

```text
input_hash  = sha256(canonical(input))
output_hash = sha256(canonical(output))
signature   = sha256(id + "@" + version + ":" + input_hash + output_hash)
```

Canonical means compact JSON, sorted map keys, explicit optionality and stable
union order. Two manifests that describe the same shape get the same hash. An
exact match requires the same id, version, input hash and output hash.

The type proves shape, not meaning. Each capability id has a short spec and a
conformance test; the registry validates the provider's output against the
schema on every call.

## Namespaces, conflicts and aliases

| Situation | Result |
| --- | --- |
| App exports `org.anth.host.*` | Rejected: reserved namespace |
| App exports another app's namespace | Rejected: id not owned |
| Same `id@version` exported twice | New export rejected; existing provider untouched; app `conflict` |
| Same shape in a different namespace | Allowed; registry records an informational equivalence |
| Import with no exact host export | App `unresolved` |
| Import of a plugin export in v1 | Rejected: `unsupported_provider` |
| Operator alias to a plugin export | Allowed from config; audited; re-approval |

An app never claims a host id. A plugin that wants to replace a host
implementation is referenced by an operator alias:

```toml
[apps.aliases]
"org.anth.host.info@1" = "org.example.dashboard.info@1"
```

The alias is configuration, not a grant field. It is visible in `app show` and
the audit log; removing it restores the host provider.

## Registry

New pure module (`apps/registry.rs` in the implementation):

1. Parse and validate every installed manifest.
2. Reject namespace violations and exact duplicates.
3. Insert the host catalog.
4. Resolve imports: exact host match, `provider = "host"`.
5. Validate that every CLI/API/UI entry maps to an own export.
6. Compute `enabled`, `disabled`, `unapproved`, `unresolved`, `conflict`, `error`.
7. Return the graph and per-app diagnostics.

The result is deterministic and independent of installation order.

### Checking a manifest

```bash
anth-panel app check --manifest manifest.toml
anth-panel app check --manifest manifest.toml --json
anth-panel app check --manifest manifest.toml --grant
anth-panel app check --manifest manifest.toml --grant --previous old-grant.toml
```

The command parses and validates the manifest, builds the host-only graph,
prints it, including the computed risk and the approval flags, and exits `0`
when every app is `enabled`. It exits `1` when an import
is unresolved or a surface conflicts, and `2` when the manifest itself is
invalid. It never installs, grants or runs anything.

`--grant` prints the normalized grant (TOML, or JSON with `--json`) instead of
the graph. It includes the app risk, per-capability risk, normalized resources
and limits, and the approved imports. It is a preview: no file is written.

`--previous old-grant.toml` (with `--grant`) prints the authority diff:
added/removed/changed capabilities, imports and risk. It is the review the
operator gets before approving an update.

v1 has no plugin-to-plugin edges. A plugin export is visible in `app show` and
usable by that plugin's own CLI/API/UI; it is not a provider for other plugins.

## Grant v3

```toml
version = 3
id = "org.anth.podman"
name = "Podman"
app_version = "0.1.0"
enabled = true
manifest_sha256 = "..."
wasm_sha256 = "..."
risk = "admin"
approved_at_ms = 1767225600000
approved_by = "uid:0"
manifest_signature = "..."

[[capability]]
id = "org.anth.podman.containers.list"
version = 1
kind = "service"
implementation_sha256 = "..."
sandbox = "resource"
resource = ["socket:/run/user/1000/podman.sock"]
limits = "mem=64MiB,pids=32,wall=30s"
risk = "admin"

[[import]]
id = "org.anth.host.info"
version = 1
provider = "host"
risk = "read"
```

The grant stores both exports and imports. Its risk is the highest capability
or import risk; a grant whose stored risk is lower than one of its capabilities
is refused.

The grant stores the normalized capability set, not the raw manifest. A field
the grant does not cover is refused at load. A change to a manifest, wasm or
helper hash changes the surface and requires the same approval again.

### Risk

| Condition | Risk |
| --- | --- |
| Host operation marked read-only | `read` |
| Host operation that mutates | `manage` |
| Wasm export with only read host imports | `read`/`manage` |
| Binary/service with no privileged resource | `manage` |
| Binary/service with socket, device or network | `admin` |
| `argv = "any"` or `sandbox = "host"` | `host` |
| System package installation | `host` (separate `--accept-package`) |

The manifest cannot set or downgrade the risk.

### Approval

| Level | Approval |
| --- | --- |
| `read`/`manage` | `--accept-capabilities` |
| `admin` | `--accept-admin` and an interactive confirmation |
| `host` | `--accept-host`, typing the app id, `[apps] allow_host_exec = true` |
| package | `--accept-package`, `[apps] allow_package_install = true` and a repository allowlist |

## CLI, API and UI entries

All three map to an export of the same app:

```toml
[[cli]]
name = "containers"
export = "org.anth.podman.containers.list"

[[api]]
method = "GET"
path = "/containers"
export = "org.anth.podman.containers.list"
```

- CLI: `anth-panel app <app-id> <name>`.
- API: `/api/apps/<app-id>/<path>`; the panel keeps auth, Origin and CSP.
  The route resolves to an export; for a `wasm` export the host validates the
  anchor envelope `{method, path, query, body}` against the export input, calls
  the export entry, and validates the reply body against the export output.
  Non-wasm exports are not served through anchors yet.
- UI: `slot` is a host enum; `tag` is `anth-<app>-...`; module is hashed and
  served from the bundle under the iframe bridge.

A `cli`/`api`/`ui` entry that points to an unknown or foreign export is an
invalid manifest. The input/output schemas come from the export; they are not
duplicated.

## Host catalog

The catalog grows with consumers; it does not publish speculative routes. The
first entry is:

| Id | Version | Risk | Operation |
| --- | --- | --- | --- |
| `org.anth.host.info` | 1 | `read` | `host.info` |

The schema is the `HostInfo` shape (hostname, os, uptime, cpu, load, memory,
collected_at_ms). The next entries land when hello/wireguard/Podman need them.

## SDK

`anth-plugin` compiles ABI 3 by default. The `abi4` feature switches
`ABI_VERSION` to 4 and `HostExecutor` to `anth_capability_call`; the app code
keeps using `Executor::exec(capability, args)`. A v3 bundle is built with
`--features abi4` and declares `abi = 4`.

## Implementation kinds

| Kind | Source | Runtime |
| --- | --- | --- |
| `host` | Rust in the panel | In-process |
| `wasm` | The app's own module | wasmi, same sandbox |
| `binary` | `helpers/` bundle, host allowlist or package | One-shot process |
| `service` | Same sources as binary | Supervised process over `http-fd` |

Native implementations:

- `source = "bundle"` lives under `helpers/`, is hashed and read-only.
- `source = "host"` is an absolute path pinned by hash and allowlisted by the
  operator.
- `source = "package"` names a package, a pinned version and a binary hash. The
  panel may install it only after `--accept-package`; never automatically.
- A service receives an inherited fd, not a host-visible socket path.
- The sandbox profiles are `read-only`, `resource`, `network` and `host`. The
  first three are contained; `host` is explicit host-admin authority.

## Migration summary

| Current | Target |
| --- | --- |
| Manifest v2, ABI 3, grant v2 | Manifest v3, ABI 4, grant v3 |
| `[bin] socket` and the bin proxy | `service`/`binary` with fds |
| `[[capability]]` program/args | `[[export]]`/`[[import]]` signatures |
| `anth-extension::serve_unix(path)` | `serve_fd(fd)` |
| Plugin-to-plugin absent | Still absent in v1; registry records exports |

The panel protocol remains `PROTOCOL_VERSION = 4`; the app ABI is what bumps.

## Phases

1. Contracts and schemas (this document).
2. Registry, host-only, exact imports, CLI/API/UI validation.
3. Grant v3, risk, approval, audit.
4. ABI 4 and host capabilities; migrate hello/wireguard.
5. Wasm exports and surfaces.
6. Binary/service implementations, sandbox, `admin`.
7. System packages with explicit approval.
8. Podman helper and fake-socket tests.
9. Hardening, attack tests, docs.
