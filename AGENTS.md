# AGENTS.md

## What this repository is

`anthracite-panel` ("Modular Server Admin Panel") is a Rust Cargo workspace. It is
intended to hold the daemon, CLI, panel, protocol and config crates, plus a web UI for
the panel.

## Current state: scaffold only

The workspace is a scaffold. The root tree contains only `Cargo.toml`,
`.cargo/config.toml`, `.gitignore` and `LICENSE` — no crates, sources, tests, CI,
Docker files or docs. `Cargo.toml` declares `members = []`.

Because the manifest is virtual with no members, there is **no working build, test or
lint command yet**:

```
$ cargo build      # also: cargo check, cargo test, cargo clippy
error: manifest path `...` contains no package: The manifest is virtual,
and the workspace has no members.
```

Add a crate and list it under `members` before any of those will run. There is no
Makefile, justfile, CI workflow or pre-commit config to follow.

## Toolchain

- Stable Rust (`cargo`/`rustc` 1.99.0 at the time of writing). No `rust-toolchain.toml`,
  so whatever stable `rustup` provides is used.
- `.cargo/config.toml` pins `build.target = "x86_64-unknown-linux-musl"`, but that
  target is not installed by default. Any build then fails with
  `error[E0463]: can't find crate for 'std' ... the x86_64-unknown-linux-musl target may not be installed`.
  Either `rustup target add x86_64-unknown-linux-musl`, or override per command with
  `--target x86_64-unknown-linux-gnu`. Keep the musl pin in mind when running anything.

## Commands

| Task   | Command |
| --- | --- |
| Build  | `cargo build [--target x86_64-unknown-linux-gnu]` |
| Test   | `cargo test --workspace [--target x86_64-unknown-linux-gnu]` |
| Format | `cargo fmt` (verify with `cargo fmt --check`) |
| Lint   | `cargo clippy --workspace --all-targets` |

Only `cargo fmt` / `cargo fmt --check` run today (trivially). The rest start working
once the workspace has members.

## Conventions

- Commit `Cargo.lock`: it is not in `.gitignore`, so dependency changes should update it.
- `.gitignore` already covers `debug`, `target`, `**/*.rs.bk`, `*.pdb`, `**/mutants.out*/`,
  `rustc-ice-*.txt` and `.idea/`. Do not commit build output.
- Keep `resolver = "2"` in the workspace manifest.
