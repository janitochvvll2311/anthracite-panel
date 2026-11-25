# Apps

Each app follows the same catalogue shape. The conventions the official apps
share — the tree, the service half, the web family (tokens, forms, modals, FAB,
responsive) and the tests — are in
[design §30](../docs/design/30-official-app-conventions.md). A new official app
should follow them; a third-party app only has to satisfy the contract in §7.

```
apps/<app>/
  backend/          Rust service (native apps; the demo apps use demo/emulator)
  frontend/         Bun/TypeScript workspace; `src/` is split into modules
    src/
    dist/<app>.js   generated module read by the bundle builder (committed)
    package.json
    tsconfig.json
  wit/              the app's WIT package
  manifest.toml.in  bundle manifest with `{{sha256}}` placeholders
```

`apps/hello` is the minimal component and keeps `src/`, `wit/`, `web/` and a
final `manifest.toml` because its `wasm/app.wasm` is committed. `apps/home`,
`apps/fs`, `apps/terminal`, `apps/proc`, `apps/net`, `apps/storage`, `apps/accounts`,
`apps/firewall`, `apps/certs`, `apps/proxy` and `apps/acme` have real Rust
backends; the eight
demo bundles use the shared
fixture in `demo/emulator` as their backend, and their `frontend/` modules are
what the demo bundles carry.

## Build the native apps

```bash
apps/home/build.sh          # real host facts
apps/fs/build.sh            # host filesystem rooted at --root (default /)
apps/terminal/build.sh      # native PTY shell
apps/proc/build.sh          # real /proc reader, signal and spawn
apps/net/build.sh           # host interfaces, routes and apply
apps/storage/build.sh       # host block devices, mounts and guarded mount/unmount
apps/accounts/build.sh      # local users, groups and guarded account changes
apps/firewall/build.sh      # managed nftables table, rules and policies
apps/certs/build.sh         # certificate inventory, custody and publication
apps/proxy/build.sh         # reverse proxy: vhosts, TLS, WebSocket, HTTP/2
apps/acme/build.sh          # ACME client: accounts, orders, HTTP-01 and DNS-01
```

The native Files, Terminal, Processes, Network, Storage, Accounts and Firewall
bundles replace the emulator bundle under the same app id; Certificates,
Proxy and ACME are new native apps with no emulator counterpart. They declare `host`,
`exec`, `admin` and `socket` service classes, so approval needs
`--accept-write --accept-admin --accept-host` according to the class (Proxy and
ACME need `--accept-write --accept-admin`).
Their `manifest.toml.in` uses
`{{app_name}}`, `{{app_summary}}`, `{{service_path}}`, `{{service_class}}` and
`{{service_argv}}`; the emulator builder and the native build scripts fill them
for their respective bundles.

## Build the frontends

```bash
apps/build-frontends.sh
```

The script installs, type-checks and bundles every `apps/*/frontend` into
`frontend/dist/<app>.js`. The demo apps commit their generated module so
`demo/emulator/bundle-emulator` can package a bundle without Bun; `apps/home`,
`apps/fs` and `apps/terminal` rebuild their module in their own build scripts.

## FAB and modal modules

An app that contributes a FAB declares it in `manifest.toml.in`; unlike a view,
the FAB is declarative and has no module. The `view` tag in this hypothetical
app must match one of its own `panel.view` tags in a real manifest:

```toml
[[slot]]
id = "panel.fab"
label = "Add"
order = 30
tag = "add-entry"
view = "anth-entry"
module = ""
sha256 = ""

[[slot.actions]]
label = "New"
tag = "new"
action = "new"

[[slot.actions]]
label = "Upload"
tag = "upload"
action = "upload"
files = true
```

A `target` action points at a `panel.modal` slot of the same app; an `action`
is a local tag delivered to the active view. The FAB trigger is icon-only when
an icon is available; its `label` is the accessible name and tooltip. Menu items
keep their labels. The view opens a modal with
`context.openModal({ tag, params })`, handles local actions with
`context.onAction(tag, payload)`, and the modal closes with `context.close()`.
A `files = true` action delivers `{ files: File[] }`
to `onAction`; the panel owns the picker so the browser user gesture survives.
After it closes, the panel sends `REFRESH` to the originating view.

Modal forms live in a second frontend entry, usually `src/modal.ts`, and are
packaged as `web/<app>-modal.js`. Its build script must include the modal in
the bundle and replace `{{modal_sha256}}`; the demo bundle builder picks up
`frontend/dist/<app>-modal.js` automatically.

Keep destructive confirmations in the app's own `<dialog>` or inline
confirmation. A panel that does not implement `panel.fab`/`panel.modal` ignores
those contributions, so an app that wants to remain usable there should keep a
local fallback control.

## Icons

An app declares its sidebar identity in `[app]`; a slot or FAB action can
override it:

```toml
[app]
icon = "home"                    # a name from the panel's set

# or an app-provided asset:
# icon = "web/icon.svg"
# icon-sha256 = "…"
```

Names include `home`, `folder`, `terminal`, `activity`, `network`, `disk`,
`users`, `shield`, `refresh`, `plus`, `upload`, `download`, `user`, `group`,
`file`, `link`, `gear`, `bolt` and `hello`. An asset must live under `web/`, use
`.svg`, `.png` or `.webp`, and carry its pinned hash. The panel renders names as
inline SVGs and assets as a CSS mask that inherits the current color.

Every file under `apps/<app>/web/` is packaged. Its hash placeholder is
`{{<stem>_sha256}}` (`icon.svg` → `{{icon_sha256}}`, `refresh.svg` →
`{{refresh_sha256}}`); the build script and the demo bundle builder replace
those placeholders and copy the files.
