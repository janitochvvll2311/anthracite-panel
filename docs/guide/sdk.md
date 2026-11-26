# App SDK

The `v0.8.0` SDK lets a third party create, build, sign and verify an
Anthracite app without cloning this repository. Rust and Bun are both first
class; the CLI is one Rust binary, and Bun gets the same commands through
`bunx`.

Design: [§31](../design/31-sdk-distribution.md). Decisions: [0013](../decisions/0013-what-is-the-public-sdk-surface-for-third-party-anthracite-apps.md)–[0018](../decisions/0018-what-exactly-does-anth-bundle-own.md).

## Install

Rust:

```sh
cargo install cargo-anth
```

The release `bin/` also carries `anth`, `anth-sign` and `anth-conformance`;
put that directory on `PATH` or copy the three binaries next to your tools.
`cargo install cargo-anth` provides `cargo anth` only.

Bun:

```sh
bun add @anth-panel/panel @anth-panel/client
# choose one:
bunx @anth-panel/cli app new org.example.notes --lang bun
# or
bunx @anth-panel/create-app org.example.notes
```

`@anth-panel/cli` runs the `anth` binary from the release (or `ANTH_BIN`), with
`cargo anth` as the fallback. There is no native `bun anth` subcommand.

## Quick start

```sh
cargo anth app new org.example.notes --lang rust
cd notes
anth-sign keygen --out dev.key
cargo anth app doctor
cargo anth app build
cargo anth app check
cargo anth app package --key dev.key
cargo anth app verify --public-key "$(anth-sign public-key --key dev.key | awk '/^public_key/ {print $2}')"
```

`--lang bun` creates the same wasm component plus a Bun panel module; `build`
runs `cargo`/`wasm-tools` and `bun build`, then updates the manifest hashes.

### Templates

| Flag | What it creates |
| --- | --- |
| `--lang rust` (default) | a Rust wasm app with one exported interface and a route |
| `--lang bun` | the wasm component plus a Bun panel module (`web/app.js`) |
| `--dir <path>` | the project directory instead of the last id label |

Both templates are standalone: their `Cargo.toml`/`package.json` never point at
this repository. The native-service template lands with the service SDK; the
generated wasm component is valid for both.

## `anth.toml`

```toml
[app]
id = "org.example.notes"
lang = "rust"          # or "bun"

[build]
target = "wasm32-unknown-unknown"
# artifact = "notes"   # optional; defaults to the Cargo package name

[wit]
catalog = "0.8"
world = "app"

[sign]
key = "env:ANTHRACITE_SIGNING_KEY"

[panel]
dev-port = 5173
```

`--project <dir>` points at the project root (or the `anth.toml`). The CLI
walks up from the current directory when it is omitted.

## Commands

| Command | What it does |
| --- | --- |
| `app new <id> [--lang rust\|bun] [--dir <path>]` | create a standalone project |
| `app doctor` | check cargo, rustc, the wasm target, wasm-tools, Bun and the signing tools |
| `app build` | compile and wrap the component; update manifest hashes |
| `app check` | build, package a temporary bundle and run `anth-conformance` |
| `app package [--key <key>]` | write `dist/<id>-<version>.anth`, signed when a key is given |
| `app sign --key <key>` | sign an existing bundle |
| `app verify [--public-key <hex>]` | run `anth-conformance`, requiring a signature when the key is given |

`cargo anth ...` and `anth ...` run the same implementation; `cargo anth` is
the cargo subcommand convention, like `cargo clippy`.

## Signing and conformance

`anth-sign` signs the canonical set (`manifest.toml` plus `wit/**`).
`anth-conformance` is the engine's offline verifier; it checks the manifest,
hashes, WIT world, exports and signature. The release `bin/` has both.

```sh
anth-sign keygen --out dev.key
anth-sign public-key --key dev.key
cargo anth app package --key dev.key
cargo anth app verify --public-key <hex>
```

A private key is never committed: the generated `.gitignore` ignores `*.key`.

## Compatibility

| SDK | Engine | WIT interfaces |
| --- | --- | --- |
| `0.8.x` | `0.8.x` | the interface versions in the installed catalog |

The SDK crate version follows the v0.8 line; WIT interfaces keep their own
versions (`anth:store/kv@1.0.0`). A breaking SDK change bumps the minor while
the line is `0.x`; a published version is immutable.

## Packages

| Rust | npm |
| --- | --- |
| `anth-error`, `anth-bundle` | `@anth-panel/wit` |
| `anth-guest`, `anth-service`, `anth-client` | `@anth-panel/client`, `@anth-panel/panel` |
| `cargo-anth` | `@anth-panel/cli`, `@anth-panel/create-app`, `@anth-panel/bun-plugin` |

The npm packages are scoped under `@anth-panel`; `anth-*` is a crates.io naming
convention, not a registry namespace. Unscoped npm aliases may be added later
as thin reexports; the scoped package is canonical.

## Examples

```sh
# A Rust wasm app with one route.
cargo anth app new org.example.rust --lang rust
cd rust
cargo anth app build && cargo anth app check

# A Bun panel module plus its wasm component.
cargo anth app new org.example.panel --lang bun
cd panel
bun install
cargo anth app build
```

The repository also carries `examples/sdk/rust` and `examples/sdk/bun`, generated
by the CLI. The release bundles `hello` and the five edge apps are complete
examples of the layout; `apps/hello` is the smallest wasm app.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `cargo anth` unknown | `cargo install cargo-anth`; cargo finds `cargo-anth` on `PATH` |
| `anth-sign not found` | put the release `bin/` on `PATH` or set `ANTH_SIGN` |
| `wasm-tools not found` | install `wasm-tools 1.263.0` or set `WASM_TOOLS` |
| `target ... not installed` | `rustup target add wasm32-unknown-unknown` |
| `cannot find package @anth-panel/...` | install the scoped package; the plugin also needs `@anth-panel/client` |
| a bundle is refused | run `anth-conformance` on it; the message names the hash, world or signature |
