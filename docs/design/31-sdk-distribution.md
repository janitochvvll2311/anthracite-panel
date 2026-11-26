# 31. SDK distribution for third parties

> Part of the [design index](../../DESIGN.md). Status: **accepted for the
> v0.8.0 line**. The public SDK, its packaging and the developer workflow for
> apps written outside this repository.

## 1. The problem

The repository is public, but a third party cannot build an app from it without
cloning the monorepo. The build scripts assume `apps/Cargo.toml`, the TypeScript
packages are private and export source, `anth-service` depends on the engine's
contract by path, and the signing/conformance binaries are not part of a
release's `bin/`.

The goal is the shortest honest path from "I have an idea" to "a signed bundle
the engine accepts":

```sh
cargo anth app new org.example.notes --lang rust
bunx @anth-panel/cli app new org.example.notes --lang bun
```

## 2. What is public

| Artifact | Registry | Audience | Stable API |
| --- | --- | --- | --- |
| `cargo-anth` (bin `cargo-anth`; `anth` in the release) | crates.io | app authors using cargo | yes |
| `anth-guest` | crates.io | wasm apps in Rust | yes |
| `anth-service` | crates.io | native services in Rust | yes |
| `anth-client` | crates.io | HTTP/CLI clients in Rust | yes |
| `anth-error` | crates.io | error kind and envelope shared by services and bundles | yes |
| `anth-bundle` | crates.io | canonical message, WIT/file hashes, zip unpack and signature | yes |
| `@anth-panel/panel` | npm | app UI in Bun/browser | yes |
| `@anth-panel/client` | npm | transport and generic client | yes |
| `@anth-panel/wit` | npm | versioned WIT catalog | yes |
| `@anth-panel/cli` | npm | `bunx @anth-panel/cli` | yes |
| `@anth-panel/create-app` | npm | `bun create @anth-panel/app` | scaffolding |
| `@anth-panel/bun-plugin` | npm | preload hooks for dev/build | experimental |
| `anth-sign`, `anth-conformance` | GitHub Releases | offline signing and verification | binaries |

The owner is **`anth-panel`**: crates.io ownership and the npm scope
`@anth-panel`.

The workspace packages are still named `@anthracite/panel` and
`@anthracite/client` with `private: true`. They are the same artifacts as
`@anth-panel/panel` and `@anth-panel/client`; the rename happens in the npm phase,
before the first publish.

The scoped names are the canonical ones. Do not publish unscoped `anth-*`
packages at launch: `anth-*` is a textual prefix, not a namespace, so anyone
could impersonate the family. If demand appears later, an unscoped alias is a
thin re-export of the scoped package and is documented as an alias; the scoped
package stays canonical.

crates.io has no scopes and does not reserve prefixes. The `anth-*` crates are
claimed one by one and owned by `anth-panel`; the convention is documented, not
enforced by the registry.

### What stays internal

`anth-api`, `anth-engine`, `anth-runtime`, `anth-compose`, the full
`anth-contract`, and `anth-panel` (Rust) are not published: they encode the
engine's authority and its implementation, not a contract for app authors.
`anth-error` is the exception: it is a small public crate extracted from
`anth-contract` and re-exported by it. The `system` tier and the manifest
verification remain the engine's job.

## 3. One CLI, two entry points

The CLI is one Rust library shared by the `cargo-anth` binary and the `anth`
client. `cargo install cargo-anth` provides `cargo anth`; the release and the
`anth-cli` binary provide `anth` with the same `app ...` commands.

- Cargo discovers `cargo-anth` on `PATH`; it invokes it as
  `cargo-anth anth app build`. The binary drops the leading `anth` that cargo
  adds. This is the same mechanism as `cargo clippy` or `cargo nextest`,
  without making Anthracite a rustup component.
- `anth ...` is the direct entry point for scripts, CI and the npm wrapper.

Bun has no subcommand registry. The integration is the Drizzle-shaped one:

- `bunx @anth-panel/cli ...` runs the CLI;
- `bun create @anth-panel/app` scaffolds a project;
- `bunfig.toml` can preload `@anth-panel/bun-plugin` for virtual `anth:client`
  modules, WIT transforms or panel HMR.

A native `bun anth` subcommand is not attempted: Bun would have to provide the
extension point, and a shell alias is not a distribution story.

## 4. Configuration

One file at the project root, `anth.toml`:

```toml
[app]
id = "org.example.notes"
lang = "rust"          # or "bun"

[build]
target = "wasm32-unknown-unknown"

[wit]
catalog = "0.8"

[sign]
key = "env:ANTHRACITE_SIGNING_KEY"

[panel]
dev-port = 5173
```

TOML is read by the Rust CLI and the Bun tooling without requiring a JS
runtime. Templates generate it; `anth doctor` validates the toolchain and the
file.

## 5. Shared bundle code: `anth-bundle`

The canonical `manifest.sig` message, the WIT/file hashes, the confined zip
unpack and the Ed25519 verification live in `anth-bundle`, which lives in the
engine tree at `engine/bundle` and is published from there. Manifest parsing
and WIT semantic validation stay in the engine; a tool that needs them calls
`anth-conformance`. This is what lets the
engine depend on it without breaking the rule that forbids `engine/* -> sdk`
([§10.1](10-workspace.md#101-the-edges-we-forbid)); `anth-sign`,
`anth-conformance` and `cargo-anth` depend on the same crate, so a signature
produced by the SDK is the one the engine verifies.

The error kind and envelope used by a service move to `anth-error` in
`engine/error`, also published. `anth-service` depends on `anth-error`, not on
the private `anth-contract`, so it can be published without exposing the engine
contract.

The extraction is a refactor of `engine/compose` and must preserve the bytes:
the signed message is a compatibility boundary, not an implementation detail.
Tests pin known vectors before and after the move.

## 6. Versioning and compatibility

- The line is `v0.8.0`; the engine, `sdk/rust` and the panel workspace carry
  `0.8.0`. The SDK crates start at `0.8.0` and follow the line.
- A WIT interface has its own version (`anth:store/kv@1.0.0`); the SDK catalog
  version is the package version (`0.8`), not the interface version.
- `0.8.x` SDK is compatible with `0.8.x` engine. A breaking SDK change bumps the
  minor while the line is `0.x`, and the compatibility matrix in the guide is
  updated in the same PR.
- A published crate/npm version is immutable. A fix is a new version; a broken
  release is yanked/deprecated, never overwritten.

## 7. Distribution mechanics

- **crates.io**: `anth-error` → `anth-bundle` → `anth-guest` → `anth-service` →
  `anth-client` → `cargo-anth`. Path dependencies also carry a `version`, and
  every crate has `description`, `license`, `repository`, `readme`, keywords and
  categories. `cargo publish --dry-run` is part of the gate.
- **npm**: ESM + `.d.ts`, `exports`, `files`, `publishConfig.access = public`,
  no `private`, no TypeScript source in the tarball. `bun pm pack --dry-run` is
  part of the gate. `bun publish --access public` is the publisher; provenance
  waits for a CI runner.
- **Binaries**: `anth`, `anth-sign` and `anth-conformance` in the GitHub
  Release `bin/`, with `SHA256SUMS`. `cargo binstall` is the fast path for Rust
  users; the npm wrapper resolves the same release asset for Bun.
- **Credentials**: the owner is `anth-panel`; tokens live outside the repo and
  never in a bundle, a log or the panel.

## 8. Non-goals

- Publishing the engine crates or the full contract.
- A native `bun anth` subcommand.
- A private package registry or a WIT registry.
- Windows/macOS native services.
- Auto-publishing from GitHub Actions before a runner exists.

## 9. Gate

A phase is done when:

1. a project created from a template in `/tmp` builds, packages, signs and
   verifies without referencing the monorepo;
2. the relevant `cargo publish --dry-run` or `bun pm pack --dry-run` is clean;
3. `cargo anth` and `bunx @anth-panel/cli` run the same command with the same
   result;
4. the docs show the exact commands and the compatibility matrix.

The implementation order is baseline, CLI, Rust crates, npm packages, docs and
publish; the phase plan lands with the v0.8.0 plan.
