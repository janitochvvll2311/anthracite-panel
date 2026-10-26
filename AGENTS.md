# AGENTS.md

## Current state

- `v0.3.0` is the rewrite branch. Phase 0 foundation is in place: a Rust workspace with `anth-wire`, `anth-host`, `anth-plugin` and `anth-panel`; config/secrets/state path mapping and atomic writes; a logger; and CI. There is no daemon, HTTP route, auth service or frontend yet.
- The plan of record is `docs/design/v0.3.0.md`. The accepted Phase 0 contracts are in `docs/design/phase-0-contracts.md`. The architecture reference for the two previous trees is `docs/design/reference-v0.1-v0.2.md`.
- Prior code lives on other refs:
  - `origin/v0.2.0` — newest previous implementation: Rust workspace `backend/` (`anth-wire`, `anth-host`, `anth-plugin`, `anth-panel`), Bun workspace `frontend/`, `plugins/hello/`, frozen `v1/`, and CI in `.github/workflows/v2.yml`.
  - `origin/main` — shipped v0.1 tree: `backend/` (`anth-proto`, `anth-panel`), Vite/Lit `frontend/`, `plugins/wireguard/`, and `docs/`.
- Read prior context without changing branches:

  ```bash
  git show origin/v0.2.0:AGENTS.md
  git show origin/v0.2.0:README.md
  git show origin/v0.2.0:docs/design/v0.2.0.md
  git show origin/main:AGENTS.md
  ```

  Those files describe those trees, not this one.

## Toolchains

- Rust, rustup and Cargo live under `/tools` (`CARGO_HOME=/tools/cargo`, `RUSTUP_HOME=/tools/rustup`), pinned by `rust-toolchain.toml` to `1.99.1` with the `x86_64-unknown-linux-musl` target, `rustfmt` and `clippy`.
- Bun lives under `/tools/bun` (`BUN_INSTALL=/tools/bun`), version 1.4.4.
- `/tools/env.sh` exports the paths. `/usr/local/bin` wrappers source it, so `cargo`, `rustc`, `rustup`, `rustfmt`, `cargo-fmt`, `cargo-clippy` and `bun` work from a non-login shell. If a wrapper is missing after an environment rebuild, run `. /tools/env.sh` first.
- The system C toolchain (`gcc`, `musl-dev`, `make`, `pkgconf`) is installed with `apk`; it is not under `/tools` and may need reinstalling after a full image rebuild.

## Build and test

Run from `backend/`. The workspace pins `x86_64-unknown-linux-musl`; a bare `cargo build` is correct.

```bash
cd backend
cargo build --workspace
cargo test --workspace
cargo test --workspace --release
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Focused checks:

```bash
cargo test -p anth-wire
cargo test -p anth-host
cargo test -p anth-panel
cargo run -q -p anth-panel -- config set version 1 --config ./config.toml
cargo run -q -p anth-panel -- config validate --config ./config.toml
```

Debug builds default to relative paths (`./config.toml`, `./anth.sock`, `./state`, `./plugins`, `./plugins.d`, `./secrets.toml`). Release defaults use `/etc/anth/*` and `/var/lib/anth/*`. `Config::load_or_create` writes a missing config; `Config::set_key` edits one dotted key through `toml_edit` and preserves comments.

## Working here

- The Phase 0 contracts are accepted; implement against them and update the file if a decision changes.
- Use `/tmp/anthracite-panel` for test scratch and manual run state, never the shared `/tmp` root and never `.tmp`.
- Docs are written in English, matching the previous trees. Keep them in sync with code as it lands.
- The root `.gitignore` covers this repo's local debug config, secrets, state and plugin directories.
- License is Apache-2.0 (`LICENSE`).
- Do not switch, merge or rebase branches unless asked. Feature branches start from and target `v0.3.0`; `main` still holds v0.1.
