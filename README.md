# Anthracite Panel

A host inspection panel built around a Unix-socket daemon, an HTTP API, a CLI
and a web UI. Apache-2.0.

## Status

`v0.3.0` is a rewrite in progress. Phase 0 foundation is in place:

- Rust workspace: `anth-wire`, `anth-host`, `anth-plugin`, `anth-panel`.
- Length-prefixed JSON wire contract with `Hello`/`Welcome`, one error type and
  explicit pagination.
- Pure `/proc` and `os-release` parsers with fixture tests.
- Config/secrets/state path mapping, atomic writes and comment-preserving
  `config set`.
- Structured logger (`human`, `json`, `off`) and CI.

The daemon, HTTP API, auth service, CLI host-info slice and frontend arrive in
the following phases. The plan is [docs/design/v0.3.0.md](docs/design/v0.3.0.md);
the accepted Phase 0 contracts are
[docs/design/phase-0-contracts.md](docs/design/phase-0-contracts.md).

## Toolchain

Rust and Bun live under `/tools` so a recreated environment keeps them:

```bash
. /tools/env.sh
```

- Rust `1.99.1`, target `x86_64-unknown-linux-musl`, `rustfmt`, `clippy`.
- Bun `1.4.4`.
- Alpine C toolchain: `gcc`, `musl-dev`, `make`, `pkgconf`.

## Build and test

```bash
cd backend
cargo build --workspace
cargo test --workspace
cargo test --workspace --release
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Debug builds default to relative paths (`./config.toml`, `./anth.sock`,
`./state`). Release builds default to `/etc/anth/config.toml` and
`/var/lib/anth`.

```bash
cd backend
cargo run -q -p anth-panel -- config set version 1 --config ./config.toml
cargo run -q -p anth-panel -- config validate --config ./config.toml
cargo run -q -p anth-panel -- config set files.write true --config ./config.toml
```

## Documentation

| Document | Contents |
| --- | --- |
| [docs/README.md](docs/README.md) | Documentation index |
| [docs/design/v0.3.0.md](docs/design/v0.3.0.md) | Architecture draft and phases |
| [docs/design/phase-0-contracts.md](docs/design/phase-0-contracts.md) | Wire, config/state, bootstrap, HTTP/CLI, logging and git contracts |
| [docs/design/reference-v0.1-v0.2.md](docs/design/reference-v0.1-v0.2.md) | Architecture of the two previous trees |
| [docs/design/protocol-changelog.md](docs/design/protocol-changelog.md) | Wire versions and the pre-1.0.0 compatibility scaffold |
