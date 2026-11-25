# Anthracite

[![ci](https://github.com/janitochvvll2311/anthracite-panel/actions/workflows/ci.yml/badge.svg)](https://github.com/janitochvvll2311/anthracite-panel/actions/workflows/ci.yml)
[![Sponsor](https://img.shields.io/badge/Sponsor-GitHub%20Sponsors-ea4aaa?logo=github)](https://github.com/sponsors/janitochvvll2311)

**A headless engine that composes apps; a panel is the application that hosts
them.**

Anthracite is a Linux platform for self-hosted software. The engine is a daemon
with no UI: it verifies, installs and composes signed app bundles, enforces the
authority an app was granted, and serves an HTTP API over a Unix socket (and
optionally TCP). A **panel** is a separate application that links the engine,
serves a web UI and mounts the contributions of the composed apps. Our official
panel is one; the SDK lets you write another.

Version **0.7.0** (tag `v0.7.0`) carries M6, the operated edge: the five apps
`certs`, `acme`, `proxy`, `containers` and `store`, manifest v4 with declared
roots/listeners/egress, sandbox L1 with per-service uids and Landlock, the
`CAP_NET_BIND_SERVICE` port capability, the service broker, the audit bridge and
the manager handoff. The `v0.6` line remains the M4–M5 proof: service apps on the
v3 manifest, streams and quotas, the `system` tier with a compiled distribution
key and a signing ceremony, the sandbox baseline and the external-bundle acid
test.

The **v0.8.0** line is the third-party SDK: one Rust CLI with `cargo anth` and
`anth`, publishable crates (`anth-error`, `anth-guest`, `anth-service`, `anth-client`,
`anth-bundle`, `cargo-anth`) and npm packages under `@anth-panel`, plus
standalone templates. The contract is
[`docs/design/31-sdk-distribution.md`](docs/design/31-sdk-distribution.md); the
packages are not published yet.

The design of record is [`DESIGN.md`](DESIGN.md); the official manual is
[`docs/guide/`](docs/guide/README.md).

## Why it is different

- **Authority is declared, granted and audited.** A bundle cannot ask for a
  capability it did not declare, and the operator sees the risk and the exact
  interfaces before approving it. A grant freezes the bytes it was given for.
- **Apps are signed, closed artifacts.** A bundle carries a manifest, a WIT
  package and a WebAssembly component (or a native service); the engine verifies
  hashes, signatures and the world before a single instruction runs.
- **The engine is not the product.** No feature logic lives in the engine: files,
  terminals, processes, network, storage and accounts are apps. The engine never
  hands a filesystem path to a wasm app.
- **The panel is a client.** The API is the boundary; the official panel links
  the engine today, and a panel in another process or another language is a
  supported shape, not a fork.

## Components

| Component | Binary / crate | What it does |
| --- | --- | --- |
| Engine | `anth-engine` (`engine/daemon`) | the headless daemon: API, registry, supervisor |
| Official panel | `anth-panel` (`panel/backend`) | links the engine, serves the shell and the slots |
| CLI | `anth` (`sdk/rust/cli`) | a thin client for the engine API |
| Signer | `anth-sign` (`sdk/rust/signer`) | signs bundles and keys for the release process |
| Conformance kit | `anth-conformance` (`sdk/rust/conformance`) | validates a bundle offline |
| SDK | `sdk/rust`, `sdk/ts` | guest bindings, panel contract, generated clients |
| Apps | `apps/` | hello and the system apps (files, terminal, processes, network, storage, accounts, home, firewall) |
| Demo | `demo/` | the deterministic host emulator behind the eight system app ids, and the release demo variants |

## Quick start

The fastest path is a pre-release variant: unpack it and run it. `core` has no
apps, `base` composes the signed system apps, `demo` composes the same eight ids
served by the host emulator, and `demo-sessions` gives every browser an
ephemeral, Turnstile-gated copy of that demo.

```sh
tar -xzf anthracite-0.6.1-demo-linux-x86_64.tar.gz
cd demo
./run.sh                 # http://127.0.0.1:8090
```

The first boot writes `state/credentials.txt`; sign in as `operator` with that
password and change it. See [Getting started](docs/guide/getting-started.md) for
the variants, the Docker images and the source build.

## Build from source

Requirements: Linux, Rust 1.99.1, Bun 1.4.4 and `wasm-tools`. The four Rust
workspaces share one target directory through the root `.cargo/config.toml`.

```sh
# Engine, SDK and panel
cargo build --manifest-path engine/Cargo.toml -p anth-engine
cargo build --manifest-path sdk/rust/Cargo.toml -p anth-cli -p anth-signer -p anth-conformance
(cd panel/frontend && bun install && bun run build)
cargo build --manifest-path panel/backend/Cargo.toml

# Tests
(cd engine && cargo test --workspace)
(cd sdk/rust && cargo test --workspace)
(cd panel/backend && cargo test --workspace)

# A signed bundle and its verification
scripts/build-app.sh apps/hello
scripts/package-app.sh apps/hello /tmp/hello.anth
./target/debug/anth-conformance /tmp/hello.anth
```

The repository guide covers the full flow, including the signing ceremony and
the release variants. The demo instance and the acid test live in
[`demo/`](demo/README.md) and [`scripts/acid-test.sh`](scripts/acid-test.sh).

## Documentation

| Where | What |
| --- | --- |
| [`docs/guide/`](docs/guide/README.md) | the official manual: install, configure, CLI, API, apps, services, signing, panels, demo, operations, security, troubleshooting |
| [`DESIGN.md`](DESIGN.md) | the design of record, with a map of every section under `docs/design/` |
| [`docs/features/`](docs/features/) | per-feature reports: what was built, why, and how it was tested |
| [`AGENTS.md`](AGENTS.md) | the working agreement for contributors and coding agents |
| [`docs/guide/cli.md`](docs/guide/cli.md) | every `anth` command |
| [`docs/guide/http-api.md`](docs/guide/http-api.md) | every HTTP/API resource |

## Repository layout

```
engine/     the platform: contract, runtime, compose, auth, api, daemon
sdk/        guest, panel, client, cli, conformance, signer (Rust); panel and client (TypeScript)
panel/      the official panel: backend binary and frontend
apps/       the app catalogue: hello and the system apps
demo/       the host emulator, the demo instance and the session launcher
tests/      e2e, fixtures and the external acid-test app
scripts/    build, package, release, images, codegen and the edge gate
docs/       design, feature reports and the guide
```

## Project status

`v0.7.0` is the current release line: manifest v4, the sandbox L1, the broker
and surface exports, and the operated edge apps. The previous `v0.6` line is
preserved as tagged releases and documentation. The project is under active
development; interfaces and bundle formats may change between minor releases.
See [`docs/guide/release-notes.md`](docs/guide/release-notes.md) for the
release history and known limitations.

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) before sending a change. Please report
security issues privately through the process in [`SECURITY.md`](SECURITY.md),
not in a public issue. Participation is covered by
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Sponsor

Anthracite is independent and has no ads or tracking. If it is useful to you,
you can support the work through
[GitHub Sponsors](https://github.com/sponsors/janitochvvll2311).

## License

Apache License 2.0. See [`LICENSE`](LICENSE).
