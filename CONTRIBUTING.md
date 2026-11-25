# Contributing to Anthracite

Thanks for helping. Anthracite is Apache-2.0 licensed and developed in the open.

## Before you start

- Read `DESIGN.md` for the architecture of record.
- Read `AGENTS.md` for the workspace, toolchain and forbidden edges.
- Read `SECURITY.md` before reporting a vulnerability. Do not open a public
  issue for security problems.

## Toolchain

The versions are part of the build contract:

- Rust `1.99.1` (`rust-toolchain.toml`);
- Bun `1.4.4`;
- `wasm-tools` `1.263.0` for app bundles;
- Linux for the engine, panel and integration tests.

Install the toolchains with `rustup`, Bun and `cargo install wasm-tools` (or the
equivalent package manager). CI uses the same versions.

## Build and test

From the repository root:

```sh
cargo build --manifest-path engine/Cargo.toml -p anth-engine
cargo build --manifest-path sdk/rust/Cargo.toml -p anth-cli -p anth-signer -p anth-conformance
(cd panel/frontend && bun install && bun run build)
cargo build --manifest-path panel/backend/Cargo.toml

(cd engine && cargo test --workspace)
(cd sdk/rust && cargo test --workspace)
(cd panel/backend && cargo test --workspace)

(cd panel/frontend && bun test)
(cd sdk/ts/client && bun test && bun run typecheck)
(cd sdk/ts/panel && bun test)
```

Run the release/acid checks when your change touches bundles, signing,
composition or the panel handoff:

```sh
scripts/acid-test.sh
```

## Pull requests

1. Fork the repository and create a focused branch.
2. Keep the change small and explain the problem, the chosen shape and the
   alternatives you rejected.
3. Add or update tests. A bug fix should have a regression test.
4. Update the design, guide or feature report when behaviour changes.
5. Run formatting, lint and the relevant tests locally.
6. Open the pull request against `main`.

Please avoid committing:

- `target/`, `dist/` or other build output (the committed app `dist` files are
  the intentional exception);
- local state, databases, uploads or credentials;
- `.env` files or secrets;
- editor-specific configuration.

Generated API clients and committed app UI modules have their own codegen
commands; do not edit them by hand unless the change is in the generator too.

## Style

- Rust: `cargo fmt` and `cargo clippy -- -D warnings` per workspace.
- TypeScript/Lit: match the existing component and test patterns.
- Commits: use a short imperative subject, optionally with a scope
  (`engine:`, `panel:`, `sdk:`, `apps:`, `docs:`).
- Do not mix refactors with behaviour changes in the same pull request.

## Security

Do not put secrets, private keys, tokens or personal data in commits, fixtures,
logs or screenshots. If you find a secret in the repository, report it through
the private security advisory channel instead of opening a public issue.
