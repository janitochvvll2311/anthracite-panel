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
final `manifest.toml` because its `wasm/app.wasm` is committed. `apps/home` has
a real Rust `backend/` (`home-service`) and a simulated `home-demo` binary for
the demo. The six demo apps use the shared fixture in `demo/emulator` as their
backend; their `frontend/` modules are what the demo bundles carry.

## Build the frontends

```bash
apps/build-frontends.sh
```

The script installs, type-checks and bundles every `apps/*/frontend` into
`frontend/dist/<app>.js`. The demo apps commit their generated module so
`demo/emulator/bundle-emulator` can package a bundle without Bun; `apps/home`
rebuilds its module from source in `apps/home/build.sh`.
