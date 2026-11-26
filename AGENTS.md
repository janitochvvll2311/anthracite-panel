# AGENTS.md

## What this is

Anthracite is a headless engine that composes apps; a panel is the application
that hosts them. Read `DESIGN.md` first: it is the architecture of record and its
decision log is binding. The user, operator and app-author manual is
`docs/guide/README.md`; it describes the shipped behaviour and links back here
and to the design where the reason matters.

## State

`v0.8.1` is the current release: a distribution update over the `v0.8.0` App SDK.
The engine and panel binaries are unchanged; the `base` variant now ships and
composes `org.anthracite.store`, and the store seeds
`https://anth-panel.com/store` as its default catalogue source on a clean install
or an empty `sources.json`. The website serves the signed catalogue at
`/store/index.json` and `/store/index.json.sig`; it lists the official apps and
the third-party versions without uploading bundles.

`v0.8.0` is the App SDK: `cargo anth` / `anth` for Rust and `bunx @anth-panel/cli`
for Bun. The six public crates (`anth-error`, `anth-bundle`, `anth-guest`,
`anth-service`, `anth-client`, `cargo-anth`) are published on crates.io and the
six `@anth-panel/*` packages on npm; the four variants, `SHA256SUMS` and the
per-app bundles are in the `v0.8.0` GitHub Release. The contract is
`docs/design/31-sdk-distribution.md` and ADRs 0013–0018; the close report is
`docs/features/app-sdk/report.md`.

`v0.7.0` is the previous release: manifest v4, sandbox L1 and the port
capability, the broker/audit/surface exports, and the five edge apps (`certs`,
`acme`, `proxy`, `containers`, `store`). The edge composition is
`compositions/edge.toml`; the removal matrix is
`engine/api/tests/golden.rs`; the release gate and manual QA are
`docs/guide/release-notes.md` and `docs/guide/edge-qa.md`. Its close report is
`docs/features/v0-7-0/report.md`.

Previous lines are preserved in the design, the guide and the feature reports;
their tags are not guaranteed to exist. Do not copy code from an old line.

## Tags

Intermediate publishes of a version append a letter: `vX.Y.Za`, `vX.Y.Zb`,
`vX.Y.Zc`, in order, with no separator (as `v0.3.0a` did). The unsuffixed
`vX.Y.Z` is the **final** publish of that version and is cut last, once the line
is done: the suffix means "not final yet", not "after the release". The letter is
the publish count, not a stability label. Do not add more `-pre.N` tags.
`v0.7.0` is the final tag of the 0.7 line; the next line starts at `v0.8.0a`.
A tag is cut from the branch it publishes and is never moved or reused.

## Toolchain

- Rust `1.99.1` (`rust-toolchain.toml`).
- Installed targets: `x86_64-unknown-linux-musl`, `wasm32-unknown-unknown` and
  `wasm32-wasip2`. **Apps build for `wasm32-unknown-unknown`**; `wasm32-wasip2`
  links WASI and every app built with it would fail the world check.
- `wasm-tools` `1.263.0` wraps the module into a component and feeds the codegen.
  `cargo-component` is not needed.
- Bun `1.4.4`.
- The four Rust workspaces share one `build.target-dir` through the root
  `.cargo/config.toml`. Do not add per-workspace targets.

## Workspaces

Four Rust workspaces and three Bun ones. They are separate on purpose: a
dependency that should not exist becomes unrepresentable instead of discouraged.

- `engine/` — the platform: `contract`, `runtime`, `compose`, `auth`, `api`,
  `daemon`, `error` and `bundle`. Builds `anth-engine`; `error` and `bundle`
  are the small public crates the SDK tools share.
- `sdk/rust/` — `guest` (wasm apps), `panel` (panels), `client` (generated
  HTTP/CLI clients), `cli` (the `anth` commands), `service` (native services),
  `conformance` and `signer` (the offline tools), `cargo-anth` (the app developer
  CLI behind `cargo anth` and `anth app ...`).
- `panel/backend/` — the official panel binary; links the SDK, mounts the engine
  router, wires the CLI and embeds `panel/frontend/dist`.
- `apps/` — app bundles. Its own workspace; never built for the host.
- Bun: `sdk/ts/`, `panel/frontend/`, `apps/*/frontend/`.

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

# the panel dev server on 127.0.0.1:5173 (ANTH_DEV_LISTEN=0.0.0.0:5173 for a preview)
scripts/dev.sh

# the external-bundle acid test: build, sign, install, compose, serve, answer
scripts/acid-test.sh

# the SDK CLI gate: a project in /tmp builds, signs and verifies
scripts/sdk-cli-test.sh

# package/dry-run/publish the public Rust SDK in dependency order
scripts/release-sdk.sh --check

# build/pack/publish the @anth-panel npm packages
scripts/release-npm.sh --check

# publish the four images to GHCR (skopeo login first)
scripts/release-ghcr.sh --images <release-images-dir> --public

# browser e2e against a running panel (tests/e2e/README.md)
(cd tests/e2e && bun install)
(cd tests/e2e && ANTH_PANEL_URL=http://127.0.0.1:5173 ANTH_PANEL_PASSWORD=... bun run test)

# a release: compile the public key in, sign every app bundle (private key local)
scripts/release.sh --key <private-key> [--out release]

# the four distribution archives from a signed release
scripts/release-variants.sh --release <signed-release-dir>

# Docker images for those variants (docker load -i <tar.gz>)
scripts/release-images.sh --variants <staged-variants-dir>

# every signed bundle as standalone assets (per-app installs)
scripts/release-bundles.sh --release <signed-release-dir> [--out <dir>]

# an app bundle (the SDK wraps these three steps)
cargo build --manifest-path apps/Cargo.toml --release --target wasm32-unknown-unknown
wasm-tools component embed --world app apps/<app>/wit <module>.wasm -o embedded.wasm
wasm-tools component new embedded.wasm -o wasm/<name>.wasm
```

A release build of the panel fails without `panel/frontend/dist`, by design.

## Rules

- No feature logic in the engine. Files, terminals, processes, network, storage
  and accounts are apps.
- The engine serves no UI, owns no slot, and never hands a filesystem path to an
  app.
- The engine must not assume its client runs in the same process.
- Docs are written in English.
- The `demo` and `demo-sessions` variants carry all eight emulated system
  bundles under the system app ids. A demo with fewer is not a release.
- Scratch state goes in `/tmp/anthracite-panel`, never the `/tmp` root.
- Do not switch, merge or rebase branches unless asked.
