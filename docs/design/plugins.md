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
web/                 optional interface assets
```

`plugin.wasm` and `plugin.toml` are required. `web/` is optional and served
under the plugin's namespace only.

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
| `hex32`, `hex64` | exactly 32/64 hex characters, as WireGuard keys are |
| `path` | absolute, normalized, no `..` |
| `enum` | one of a fixed list the host already knows |
| `text` | printable, bounded, no control characters |

`text` is the weakest and is refused for fields that reach a program. It exists
for display values a capability can pass as an opaque argument only when the
manifest's capability is marked `accepts_text = true`, which the installer
warns about.

### Installation freezes the recipe

`install` reads the manifest, validates every capability against the host
allowlist and the validator set, and writes the **normalized** capability list
into the config together with the wasm hash:

```toml
[[plugins.grants]]
id = "org.anth.wireguard"
version = "0.1.0"
wasm_sha256 = "..."
capabilities = ["wg.status"]
```

At every load the hash is recomputed. A mismatch disables the plugin and
leaves it with no capabilities, whatever the manifest says. A version bump that
declares new capabilities needs an explicit `plugin approve`.

## Sandbox

`wasmi`: pure Rust, no JIT, no `unsafe`, no WASI. The module therefore has no
filesystem, clock or randomness except what the host hands it. Static musl
builds stay trivial, which is the repository rule.

Limits: a fuel budget (default 50M per invocation, configurable) and a memory
ceiling. An invocation runs on a blocking worker so a busy plugin cannot stall
the connection loop. There is no wall-clock kill for wasm, so fuel *is* the
guarantee against a runaway loop.

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
{"kind": "cli", "argv": ["status"]}
```

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

The installer refuses a path that is not relative, contains `..`, or would
escape the namespace. Without this, a plugin could declare `/api/auth/login` and
collect credentials. The same rule covers web assets:
`/api/plugins/<id>/web/*` is confined to the bundle's `web/` directory and
served with a fixed MIME type and `nosniff`.

## UI anchors

`slot` names a place the shell already knows how to fill:

| Slot | Placement |
| --- | --- |
| `dashboard.widget` | a card in the dashboard grid |
| `panel.<name>.tab` | an extra tab inside an existing panel |
| `taskbar.item` | an item in the taskbar |
| `page` | a top-level route |

The element is created from `module`. Assets share the panel's origin, so a
plugin's JavaScript runs with the user's session and could call core `/api`
routes directly. The wasm sandbox protects the daemon from the *server* half of
a plugin, not the browser from the *client* half. Installing already requires
root, and this is documented rather than papered over; a future version can
move bundles to a sandboxed iframe with a message bridge.

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
| Plugin replaces its wasm to gain rights | hash checked on every load, mismatch disables it |
| Plugin hijacks a core route | namespace `/api/plugins/<id>/` enforced |
| Plugin escapes the bundle dir | archive member guard reused from `files/archive.rs`, plus `O_NOFOLLOW` |
| Plugin hangs the daemon | fuel budget, blocking worker |
| Plugin reads host secrets | no WASI: no filesystem, no env, no clock |
| Plugin impersonates another | unique id, duplicate registration refused |
| Zip bomb at install | same limits as file extraction |

## Phases

| Phase | Scope |
| --- | --- |
| 8a | manifest, validators, binary allowlist, install/list/remove/approve, grants in config, hash re-verification |
| 8b | `wasmi` host, ABI, fuel and memory limits, blocking execution, WAT test guest |
| 8c | API anchors and namespacing |
| 8d | UI anchors, asset serving |
| 8e | WireGuard plugin: manifest, wasm, web components |
