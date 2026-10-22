# Plugins

A plugin extends the panel without giving away the host. The model is a
kitchen:

| Role | Component | Can do | Cannot do |
| --- | --- | --- | --- |
| Chef | the daemon | run system programs | be replaced by a plugin |
| Recipe | the manifest | declare which commands it needs, and their shape | change after installation |
| Helper | the plugin (`wasm`) | send declarative data for those recipes | name a program, add a flag, touch the host |

The load-bearing consequence: **a plugin never sends an argv**. If it did, the
schema would be decoration — a plugin granted `wg` could ask for
`["set", "wg0", "private-key", "/etc/shadow"]`, or `["show", "wg0",
"private-key"]` to read a key out. So the manifest declares the *template*, the
host fills it from values it validates itself, and the template is frozen at
install time.

The validators belong to the host, never to the plugin. A manifest that could
carry a regular expression would ship `.*` and undo the schema.

## Bundle

One zip:

```
plugin.toml          manifest
plugin.wasm          server module (sandboxed, no WASI)
plugin.sig           detached Ed25519 signature of plugin.wasm (required by default)
web/                 optional interface assets
```

`plugin.wasm` and `plugin.toml` are required. `web/` is optional and served
under the plugin's namespace only. `plugin.sig` is required by default; see
[Installation](#installation-freezes-the-recipe).

## Manifest

```toml
[plugin]
id = "org.anth.wireguard"     # reverse-DNS, unique, frozen at install
version = "0.1.0"
name = "WireGuard"
abi = 1

[wasm]
path = "plugin.wasm"
sha256 = "..."                # required: this is what actually runs

[[capability]]
id = "wg.status"
program = "wg"                        # must be on the host allowlist
argv = ["show", "{interface}", "dump"] # only placeholders are substitutable
[capability.args.interface]
type = "ifname"                       # a host validator, not a plugin regex

[[api]]
method = "GET"
path = "/status"                      # mounted at /api/plugins/org.anth.wireguard/status

[[ui]]
slot = "dashboard.widget"             # or panel.network.tab, taskbar.item, page
tag = "anth-wg-card"
module = "web/wg-card.js"
```

### Argument validators

Every placeholder names a type. The host owns the set; a plugin cannot add one.

| Type | Accepts |
| --- | --- |
| `ifname` | 1–15 bytes, no `/`, no `..`, no whitespace |
| `hostname` | 1–253 bytes of letters, digits, `-` and `.`, no leading `-` |
| `port` | 1–65535 |
| `ipv4`, `ipv6`, `cidr` | a literal address or prefix, parsed |
| `hex32`, `hex64` | exactly 32/64 hex characters |
| `base64_key` | standard base64 that decodes to exactly 32 bytes, which is a WireGuard key |
| `cidr_list` | one or more `cidr` values separated by commas, no spaces |
| `path` | absolute, normalized, no `..` |
| `enum` | one of a fixed list the host already knows |
| `text` | printable, bounded, no control characters |

Every validator, including `text`, also rejects a value that starts with `-`.
A value that looks like an option is read by the program as one, which is
exactly the flag a plugin was not granted: `wg show --force`. The check sits in
the shared prelude rather than in each type, so a validator added later inherits
it instead of reopening the hole.

`text` is the weakest and is refused for fields that reach a program. It exists
for display values a capability can pass as an opaque argument only when the
manifest's capability is marked `accepts_text = true`, which the installer
warns about.

### Optional arguments

`optional = true` means the caller may omit the value. The element is then
dropped from the argv entirely rather than rendered empty, because an empty
element is itself an argument the program sees. The host only accepts this when
the placeholder is the **whole** argv element:

```toml
argv = ["show", "{interface}", "{protocol}"]   # ok

argv = ["show", "endpoint={endpoint}"]         # refused at install
```

The second form has no rendering that both omits the value and keeps the
element: it would produce `endpoint=` and reach the program as a malformed
operand. A flag that takes a separate value therefore needs one capability per
shape, which is how the reference plugin models `wg set ... endpoint <value>`.

### Signatures

`plugin.sig` is a detached Ed25519 signature over the raw bytes of
`plugin.wasm`, written as 128 hex characters. The host stores the matching
public key as 64 hex characters in the config:

```toml
[plugins]
enabled = true
install_from_web = false         # true in the generated debug config
require_signature = true         # default
public_key = "…64 hex…"
```

Install verifies the signature before the bundle is renamed into place. A
missing key, a missing `plugin.sig` or a mismatch is refused. Setting
`require_signature = false` allows an unsigned bundle and returns a warning that
the CLI prints and the plugin manager displays; a signature that is present is
still verified, so the bypass cannot turn a wrong signature into an accepted
one. `plugin approve` applies the same check before it widens a grant. Signing
is packaging work, not something the daemon does: the reference plugin's
`package.sh` uses `openssl` when `ANTH_PLUGIN_SIGNING_KEY` is set.

### Installation freezes the recipe

`install` reads the manifest, validates every capability against the host
allowlist and the validator set, and writes the **normalized** capability list
into the config together with the wasm hash:

```toml
[[plugins.grants]]
id = "org.anth.wireguard"
version = "0.1.0"
wasm_sha256 = "..."
enabled = true

[[plugins.grants.recipes]]
id = "wg.status"
program = "wg"
argv = ["show", "{interface}", "dump"]
accepts_text = false

[plugins.grants.recipes.args.interface]
type = "ifname"
optional = false
```

A grant written before the recipes existed carries `capabilities = ["..."]`.
`PluginGrant` denies unknown fields, so that config fails to load with the stale
key named, instead of deserializing into an empty recipe list and leaving the
plugin installed, listed, and unable to run anything.

The grant freezes the whole **recipe** — program, argv template and typed
arguments — not only the id. `invoke` renders argv from the grant, so a grant
holding ids alone would leave the program and the template to be read from the
manifest, which is the file an attacker rewrites. `allowed_ids()` is a view for
the CLI and the panel; `recipe(id)` is what execution uses.

At every load the module is hashed and compared **against the grant**, never
against the manifest. That distinction is the whole freeze: a manifest lives
inside a bundle an attacker can write, so a rewritten module travelling with a
rewritten `sha256` would stay self-consistent while the grant still named the
old digest. The grant is the only digest the admin approved.

A mismatch disables the plugin. The advertised capability list also comes from
the grant, never from the manifest, so a plugin that adds a capability to its own
manifest is not even *shown* as holding it. A version bump that declares new
capabilities needs an explicit `plugin approve --accept-capabilities`, and
`approve` hashes the module before recording anything.

## Sandbox

`wasmi`: pure Rust, no JIT, no `unsafe`, no WASI. The module therefore has no
filesystem, clock or randomness except what the host hands it. Static musl
builds stay trivial, which is the repository rule.

Limits, because fuel alone is not one:

- a fuel budget (default 50M per invocation, configurable) for guest
  instructions;
- a 64 MiB ceiling on the guest's linear memory;
- 32 capability calls per invocation. Fuel counts instructions, and one
  `anth_exec` costs a handful of them for one subprocess, so a loop over a
  granted capability is an amplifier, not a cost: measured at 100k spawns in 7 s
  with fuel never exhausted;
- 64 KiB of `anth_log` output, since the log is a host-side `Vec` outside
  `StoreLimits`. Past the cap the host drops lines and reports how many, rather
  than returning a log that looks complete;
- a 30 s deadline, checked between host calls, for a guest blocked on a slow
  child process.

An invocation runs on a blocking worker so a busy plugin cannot stall the
connection loop.

### ABI

Four exports, two imports. Language-agnostic: Rust, TinyGo and AssemblyScript can
all produce it.

```wat
;; exports
anth_alloc(len: i32) -> i32           ; host calls this to write a request
anth_handle(ptr: i32, len: i32) -> i64 ; returns (ptr << 32) | len
anth_free(ptr: i32, len: i32)         ; release the response
anth_abi_version() -> i32             ; must equal the manifest's `abi`

;; imports, all provided by the host
anth_exec(cap_ptr, cap_len, args_ptr, args_len) -> i64   ; JSON in, JSON out
anth_log(ptr, len)                                       ; goes to the daemon log
```

`anth_handle` receives one JSON envelope and dispatches on `kind`, so a new
anchor never adds a new export:

```json
{"kind": "http", "method": "GET", "path": "/status", "query": {}, "body": null}
{"kind": "http", "method": "POST", "path": "/peers", "query": {}, "body": "{\"interface\":\"wg0\"}"}
{"kind": "cli", "argv": ["status"]}
```

A non-empty `body` arrives as a JSON **string** holding the raw request bytes,
not as a parsed object. The host does not parse an anchor body because an anchor
is not required to speak JSON: uploading or streaming a non-JSON payload is a
legitimate use, and a host that parsed first would reject it before the guest
could decide. A guest that wants an object parses the string itself; a guest
that assumes an object sees `null`-like `as_object()` and refuses every real
request. That mismatch is exactly what shipped in the first WireGuard bundle,
which is why the reference plugin parses a string body and the integration test
drives the route through the real router rather than a hand-built envelope.

`anth_exec` takes a capability id and an object of arguments:

```json
{"interface": "wg0"}
```

The host looks the capability up in the frozen grant, validates each argument
against its declared type, substitutes the placeholders, and runs the program
through `SystemRunner` with an argument vector — never a shell. The plugin
cannot widen this call: an unknown capability id, an unknown argument name, or
a value failing its validator is an error returned as JSON.

## API anchors

A plugin may answer HTTP under its own namespace only:

```
/api/plugins/<id>/<path>
```

`anchor_route` resolves a declared path against the plugin id and refuses one
that is not relative, contains `..` as a segment, or starts with a second slash.
Without this, a plugin could declare `/api/auth/login` and collect credentials.
`Manifest::parse` applies the rule, so `install` and `load` both apply it.

It is worth being precise about what that check is and is not. It runs over the
manifest even though the manifest is inside the bundle. It is **not** how the
HTTP handler decides what a plugin may answer: the handler reads no anchors at
all and forwards every method and path under `/api/plugins/<id>/` to the guest,
which can only act through `anth_exec` against the frozen grant. The check
therefore protects the operator's expectations and the UI's catalogue — a plugin
that declares a core route is refused instead of being displayed as offering one
— and does not pretend to be an authorization boundary. A real one would have to
read the manifest on every request, which would make the untrusted file the
authority.

The same rule covers web assets: `/api/plugins/<id>/web/*` is confined to the
bundle's `web/` directory and served with a fixed MIME type and `nosniff`.

That route is deliberately not an anchor. An anchor is a request forwarded to
the guest, and answering one for a static file would mean running wasm to read a
file it cannot read. So `plugins/web.rs` reads the file itself: the grant is the
gate (a bundle directory without one serves nothing, and a disabled plugin is
`403`), the requested path is rebuilt component by component so `..` in any
spelling is refused, and the file is `canonicalize`d before the containment
check so a symlink planted after install is resolved rather than followed. The
content type comes from a host-owned extension table, `nosniff` is always sent,
and the answer is `no-store` — a cached module would outlive an uninstall, and a
reinstall reuses the same id.

## UI anchors

The shell fills slots with `anth-plugin-slot`. The shell renders one wherever a
plugin may appear and passes it the catalogue; the element asks
`lib/plugins/plugin-client.ts` for the anchors matching its slot, imports each
module with a dynamic `import()`, and appends the declared tag. The shell
therefore holds no plugin tag and no plugin import: a plugin is data — an id, a
slot, a tag, a module — until the slot element runs. A module that throws on
import is logged with its plugin id and skipped, so one broken bundle cannot
empty the dashboard.

`slot` names a place the shell already knows how to fill:

| Slot | Placement | Mounted today |
| --- | --- | --- |
| `dashboard.widget` | a card in the dashboard grid | yes |
| `panel.<name>.tab` | an extra tab inside an existing panel | no |
| `taskbar.item` | an item in the taskbar | no |
| `page` | a top-level route | no |

Only the slots marked as mounted have an `anth-plugin-slot` in the shell. A
declared anchor in any other slot is accepted by the manifest and never
rendered, which is silent: nothing logs, nothing fails, and the plugin simply
does not appear. The reference bundle hit this with `panel.network.tab`, and
`tests/plugin_wireguard.rs` now asserts that every declared tag is registered
by the module it names. The row stays in the table because the naming scheme
is the contract a future shell slot will use; it is not an invitation to
declare one.

Two more rules the reference bundle follows:

- Every `tag` must be defined by the `module` it names, via
  `customElements.define('<tag>', …)`. A declared tag nobody defines becomes an
  `HTMLUnknownElement`: an empty box with no error.
- The `module` is imported once per realm, so two anchors may share one module
  but a module may not register two elements under the same name.

The element is created from `module`. Assets share the panel's origin, so a
plugin's JavaScript runs with the user's session and could call core `/api`
routes directly. The wasm sandbox protects the daemon from the *server* half of
a plugin, not the browser from the *client* half. Installing requires an
authenticated panel session and an enabled install switch, and this is
documented rather than papered over; a future version can move bundles to a
sandboxed iframe with a message bridge.

## Browser install

`POST /api/plugins/install` takes the zip as the raw request body and runs the
same `install_bundle` as the CLI. It is gated by `[plugins] install_from_web`
(off in release, on in the generated debug config) and by `[plugins] enabled`.
The route installs on a blocking worker, writes the new grant through
`AppState::replace_config` and persists the config; a refused bundle leaves the
previous config untouched and the staged directory removed.

`anth-plugin-manager` reads `GET /api/plugins`, lists the installed bundles and
the problems the registry reported, and offers the install control only when
both switches are on. The control is behind the same dangerous-action warning
the CLI documents: the operator must confirm that the bundle runs code with the
daemon's privileges. The warning is per action and never persisted.

## CLI anchors

A plugin declares subcommands it answers; the core's dispatcher declines them
and the chain forwards to the plugin. The plugin receives `{"kind": "cli"}` and
can only act through `anth_exec`. It never receives raw socket `Request`
values: that would expose the whole protocol and defeat the model.

## Threat summary

| Threat | Guard |
| --- | --- |
| Plugin runs a program the admin did not approve | capability allowlist frozen at install |
| Plugin smuggles arguments | argv template with typed placeholders, filled by the host |
| Plugin replaces its wasm to gain rights | module hashed on every load against the **grant**, never the manifest, so a rewritten module with a rewritten manifest hash is still caught |
| A publisher is impersonated, or a module is swapped in transit | `plugin.sig` is verified against `[plugins] public_key` at install; the bypass prints a warning and still rejects a wrong signature |
| A hostile bundle is installed from the browser | route gated by `install_from_web`, install has a confirmation naming the code-execution risk, and the same zip/manifest/signature checks as the CLI |
| Plugin claims capabilities it was not granted | advertised set comes from the grant, not the manifest |
| Plugin smuggles an option instead of an operand | every validator rejects a leading `-` |
| An id with `..` reaches a recursive delete | ids are validated inside `plugin_dir`, before the join |
| Plugin hijacks a core route | namespace `/api/plugins/<id>/` enforced |
| Plugin escapes the bundle dir | archive member guard reused from `files/archive.rs`, plus `O_NOFOLLOW` |
| Plugin hangs the daemon | fuel budget, 32 calls per invocation, 30 s deadline, blocking worker |
| Plugin exhausts host memory through its log | 64 KiB log cap, dropped lines reported |
| Plugin reads host secrets | no WASI: no filesystem, no env, no clock |
| Plugin impersonates another | unique id, duplicate registration refused |
| Zip bomb at install | same limits as file extraction |

## Phases

| Phase | Scope |
| --- | --- |
| 8a | manifest, validators, binary allowlist, install/list/remove/approve, grants in config, hash re-verification — done |
| 8b | `wasmi` host, ABI, fuel and memory limits, blocking execution, WAT test guest — done |
| 8c | API anchors and namespacing — done |
| 8d | UI anchors, asset serving — done |
| 8e | WireGuard plugin: manifest, wasm, web components — done |
| 8f | Ed25519 signatures, the `require_signature` bypass, browser install and `anth-plugin-manager` — done |
