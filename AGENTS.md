# AGENTS.md

## What this is

Anthracite is a headless engine that composes apps; a panel is the application
that hosts them. Read `DESIGN.md` first: it is the architecture of record and its
decision log is binding.

## State

`v0.5.0` is a blank tree plus the design documents and this file. Nothing is implemented
yet. The next milestone is M0 (`docs/design/11-roadmap.md`).

`v0.4.0` is frozen at tag `v0.4.0-freeze`. It is reference only: do not copy its
code, its crates or its structure.

## Toolchain

- Rust `1.99.1` (`CARGO_HOME=/tools/cargo`, `RUSTUP_HOME=/tools/rustup`).
- Installed target: `x86_64-unknown-linux-musl` only. Components need
  `wasm32-wasip2` plus a component toolchain (`cargo-component` or `wasm-tools`);
  install them before starting M0.
- Bun `1.4.4` under `/tools/bun` (`BUN_INSTALL=/tools/bun`).
- If a wrapper is missing after an environment rebuild, run `. /tools/env.sh`.
- The three Rust workspaces share one `build.target-dir` through the root
  `.cargo/config.toml`. Do not add per-workspace targets.

## Workspaces

Four Rust workspaces and three Bun ones. They are separate on purpose: a
dependency that should not exist becomes unrepresentable instead of discouraged.

- `engine/` — the platform: `contract`, `runtime`, `broker`, `compose`,
  `api`, `daemon`. Builds `anth-engine`, the headless daemon.
- `sdk/rust/` — `guest` (wasm apps), `panel` (panels), `cli` (the `anth`
  commands).
- `panel/backend/` — the official panel binary; links the SDK, mounts the engine
  router, wires the CLI and embeds `panel/web/dist`.
- `apps/` — app bundles. Its own workspace; never built for the host.
- Bun: `sdk/ts/`, `panel/web/`, `apps/*/web/`.

The forbidden edges are listed in `docs/design/10-workspace.md` and checked in CI with
`cargo metadata`. `panel/backend` must not depend on `engine/*` directly, and apps
must not depend on the panel SDK.

## Commands

```bash
(cd engine && cargo test --workspace)
(cd sdk/rust && cargo test --workspace)
(cd panel/backend && cargo test --workspace)

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings   # inside each workspace

bun install && bun run build                            # from the repo root
cargo build --manifest-path apps/Cargo.toml --release --target wasm32-wasip2
```

A release build of the panel fails without `panel/web/dist`, by design.

## Rules

- No feature logic in the engine. Files, terminals, processes, network, storage
  and accounts are apps.
- The engine serves no UI, owns no slot, and never hands a filesystem path to an
  app.
- The engine must not assume its client runs in the same process.
- Docs are written in English.
- Scratch state goes in `/tmp/anthracite-panel`, never the `/tmp` root.
- Do not switch, merge or rebase branches unless asked.
