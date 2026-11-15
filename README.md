# Anthracite

**A headless engine that composes apps; a panel is the application that hosts
them.**

Anthracite is a Linux platform for self-hosted software. The engine is a daemon
with no UI: it verifies, installs and composes signed app bundles, enforces the
authority an app was granted, and serves an HTTP API over a Unix socket (and
optionally TCP). A **panel** is a separate application that links the engine,
serves a web UI and mounts the contributions of the composed apps. Our official
panel is one; the SDK lets you write another.

Version **0.6.1** (tag `v0.6.1`, the final publish of the 0.6 line; `v0.6.1a`
was the intermediate) carries M4–M5: service apps on the v3 manifest, streams
and quotas, the `system` tier with a compiled distribution key and a signing
ceremony, the sandbox baseline, and an external-bundle acid test. It also
packages the same build as variants, Docker images and standalone bundles.
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

Requirements: Linux, Rust 1.99.1, Bun 1.4.4 and `wasm-tools`. The three Rust
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

`v0.6.0` is the final tag of its line and the code is complete through M5;
`v0.6.1` publishes the same build with the official guide, the standalone
bundles and the KolibSoftware registry images (`v0.6.1a` was the intermediate).
The release process is implemented and exercised with signed release artifacts,
but three
operational items remain, and they are not code: GitHub Actions has had no
runner (every job fails in about two seconds with `runner_id: 0`), no real
long-lived release key has been used yet (the pre-release key is a rehearsal),
and the native services have not been exercised on a deployment host.

## License

Apache License 2.0. See [`LICENSE`](LICENSE).
