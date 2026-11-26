# Report — Anthracite v0.8.0 / App SDK

## Summary

`v0.8.0` turns the public repository into an installable SDK for third-party
apps. Six phases ran as one PR each into `v0.8.0`, plus a post-release state
phase, and the final tag `v0.8.0` is cut at `94f08d8`. The engine and the five
edge apps are unchanged; the line adds the authoring path:

- `cargo anth` / `anth` for Rust, from the `cargo-anth` crate.
- `bunx @anth-panel/cli` and `@anth-panel/create-app` for Bun.
- Publishable crates and npm packages under the `anth-panel` owner.

The contract is [§31](../../design/31-sdk-distribution.md) and ADRs 0013–0018.

## Integrated phases

| Phase | PR | State | Merge |
| --- | --- | --- | --- |
| 1 — SDK baseline and distribution contract | [#228](https://github.com/janitochvvll2311/anthracite-panel/pull/228) | merged | squash |
| 2 — `anth` and `cargo anth` CLI with templates | [#230](https://github.com/janitochvvll2311/anthracite-panel/pull/230) | merged | squash |
| 3 — `anth-bundle` and publishable Rust crates | [#231](https://github.com/janitochvvll2311/anthracite-panel/pull/231) | merged | squash |
| 4 — npm packages and Bun plugin | [#232](https://github.com/janitochvvll2311/anthracite-panel/pull/232) | merged | squash |
| 5 — Templates, examples and documentation | [#233](https://github.com/janitochvvll2311/anthracite-panel/pull/233) | merged | squash |
| 6 — Publish and release | [#234](https://github.com/janitochvvll2311/anthracite-panel/pull/234) | merged | squash |
| 7 — Post-release state | [#235](https://github.com/janitochvvll2311/anthracite-panel/pull/235) | in review | squash |

## Delivered

### Rust

- `cargo-anth` provides `cargo anth` and the `anth` binary; the release also
  ships `anth-sign` and `anth-conformance`.
- `anth app new|doctor|build|check|package|sign|verify` takes a standalone
  project to a signed bundle, with `anth.toml` as the only configuration.
- `anth-bundle` owns the canonical signed message, the WIT/file hashes, the
  confined zip unpack and Ed25519 verification; `anth-error` is the small error
  model both the engine and the SDK tools depend on.

### Bun

- `@anth-panel/panel` is the UI contract, `@anth-panel/client` the generated
  clients, `@anth-panel/wit` the versioned host catalog.
- `@anth-panel/cli` runs the `anth` binary with `ANTH_BIN` and `cargo anth`
  fallbacks; `@anth-panel/create-app` scaffolds a project.
- `@anth-panel/bun-plugin` resolves the virtual `anth:client` module in
  `Bun.build`.

### Published artefacts

| Channel | Versions | Notes |
| --- | --- | --- |
| crates.io | `anth-error`, `anth-bundle`, `anth-guest`, `anth-service`, `anth-client`, `cargo-anth` at 0.8.0 | published in dependency order |
| npm | `@anth-panel/{panel,client,wit,cli,create-app,bun-plugin}` 0.8.0 | `latest`, public |
| GHCR | `ghcr.io/janitochvvll2311/anthracite-panel:{core,base,demo}` and `ghcr.io/janitochvvll2311/anthracite-demo:{main}` | public; immutable `0.8.0-*` tags |
| GitHub Release | `v0.8.0` | four variants, 21 per-app bundles, `SHA256SUMS` |

The signed release is built with the distribution key at fingerprint
`300ac4479876c9f07c9d037ec2e25c18c4bc52de09a69be5fefda0c2ef2d2801`.

## Verification

- `cargo test --workspace` in `engine/`, `sdk/rust/` and `panel/backend/`;
  `cargo fmt --all --check` and `clippy -D warnings`.
- `scripts/sdk-cli-test.sh`: a project in `/tmp` builds, signs and verifies.
- `scripts/release-sdk.sh --check` and `scripts/release-npm.sh --check`.
- `scripts/release.sh` with the real key: key fingerprint checked by the built
  engine, 14 bundles signed, `manifest.json` and `SHA256SUMS` written.
- `scripts/release-variants.sh`, `scripts/release-images.sh` and
  `scripts/release-ghcr.sh`: four variants, four images, moving and immutable
  tags.
- Public installs: `bun add @anth-panel/panel @anth-panel/client
  @anth-panel/wit @anth-panel/bun-plugin` resolves at 0.8.0; `skopeo inspect`
  without credentials reads every GHCR tag.

## Publication notes

- crates.io enforced a burst limit after five new crates; `cargo-anth` was
  published a few minutes later, once the window reopened.
- The account has 2FA for writes, so npm staged the versions as
  `0.0.0-stage` placeholders until they were approved in the npm UI; `latest`
  now points at `0.8.0` on every package.
- GitHub refuses the package visibility `PATCH` for this token type (404), so
  the two GHCR packages were made public from their settings pages;
  `release-ghcr.sh` now prints those URLs instead of failing.

## Known limitations

- Bun 1.4.4 does not apply a `bunfig.toml` preload plugin to build resolution;
  register `@anth-panel/bun-plugin` in `Bun.build` explicitly.
- The native-service template lands with the service SDK; the current templates
  are Rust wasm and Bun panel.
- The app wasm is not bit-for-bit reproducible across clean builds, so the
  committed `hello` wasm is kept as tagged and the release bundles are the
  authoritative artefacts.
- Publication is manual until a CI runner exists; the scripts are the contract.

## Out of scope

- Engine or runtime behaviour changes; the contract is unchanged and the new
  packages are additive.
- Unscoped npm aliases; `@anth-panel/*` stays canonical.
- Publishing the engine crates (`anth-api`, `anth-engine`, `anth-runtime`,
  `anth-compose`, the full `anth-contract`, the Rust `anth-panel`).
