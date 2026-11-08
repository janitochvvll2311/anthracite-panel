# 10. Workspace

> Part of the [design index](../../DESIGN.md).

Four Rust workspaces and three Bun ones. They are separate on purpose: a
dependency that should not exist becomes unrepresentable instead of merely
discouraged.

```
engine/                    Rust workspace: the platform
  Cargo.toml
  wit/                     anth:env, anth:store, anth:error
  contract/                manifest, composition, grants, errors (no I/O)
  runtime/                 the only crate that names wasmtime; world checks
  broker/                  host interface dispatch, quotas, denials
  compose/                 registry, grant store, resolver, surface projection
  auth/                    operator credentials, sessions, password gates
  api/                     router, sessions, origin, assets, UDS + TCP
  daemon/                  lib + bin `anth-engine` (headless)

sdk/                       the compatibility boundary
  rust/
    guest/                 bindings for wasm apps; packages the host WIT
    panel/                 panel contract: slots, descriptors, asset URLs
    client/                generated Rust HTTP and CLI clients
    cli/                   the commands; builds the `anth` binary
    conformance/           bin `anth-conformance`; the kit third parties run
  ts/
    panel/                 iframe bridge, slot mounting, base components
    client/                generated TypeScript client

panel/                     the official implementation
  backend/                 Rust workspace: bin `anth-panel`, embeds frontend/dist
  frontend/                Bun workspace: the web app source

apps/                      backend-frontend app catalogue
  hello/                   the smallest component: one export, one import, one route
  fs/ terminal/ proc/ net/ storage/ accounts/ manager/     (v0.6.0)

demo/                      (v0.6.0) a demo instance: config, composition and demo apps

tests/
  e2e/                     Playwright against a running panel
  fixtures/                golden compositions and bundles

scripts/
  codegen.ts               WIT -> CLI commands, Rust client, TS client
  dev.sh
  package.sh
```

The `anth` binary comes from `sdk/rust/cli`; the official distribution ships it
next to `anth-panel` instead of having the panel compile it, so a headless
deployment still has a client.

The normative artifacts are `engine/wit/` and the documents under `docs/design/`.
There is no separate `docs/contracts/`: a second copy of the contract is a second
truth.

### 10.1 The edges we forbid

```
engine/*        <-  sdk/rust/panel  <-  panel/backend
engine/*        <-  sdk/rust/guest  <-  apps/*
engine/*        <-  sdk/rust/cli
sdk/ts/panel    <-  panel/frontend,  apps/*/web

forbidden:  panel/backend -> engine/*        (only through the SDK)
forbidden:  engine/*      -> sdk, panel, apps
forbidden:  apps/*        -> sdk/rust/panel
```

`engine/contract` has no dependencies on the rest of the engine; everything else
depends on it. That direction is the architecture.

The edges are checked in CI with `cargo metadata` and fail the build. A rule that
only lives in a document is a rule that will be broken.

### 10.2 One target directory

The engine, SDK and panel workspaces share `build.target-dir` through the root
`.cargo/config.toml`. Without it, `wasmtime` is compiled three times and every
full build pays for it. Separate workspaces buy isolation, not separate compilers.

`apps/` is its own workspace and is never built for the host: `cargo build
--workspace` in the root workspaces must not produce wasm artifacts. Apps build
for `wasm32-unknown-unknown` and are wrapped with `wasm-tools` ([§7.1](07-app.md#71-bundle-layout)).

---
