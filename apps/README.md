# Apps

Each app follows the same catalogue shape:

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
`apps/fs` and `apps/terminal` have real Rust backends; the seven demo bundles use
the shared fixture in `demo/emulator` as their backend, and their `frontend/`
modules are what the demo bundles carry.

## Build the native apps

```bash
apps/home/build.sh          # real host facts
apps/fs/build.sh            # host filesystem rooted at --root (default /)
apps/terminal/build.sh      # native PTY shell
```

The native Files and Terminal bundles replace the emulator bundle under the same
app id. They declare `host` and `exec` service classes, so approval needs
`--accept-write --accept-admin --accept-host`. Their `manifest.toml.in` uses
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
