# Release notes

## 0.8.1 — the Store in base

`v0.8.1` is a distribution update. The engine and the panel binaries are
unchanged; the release makes the store part of the clean-install path.

- The `base` variant now ships and composes `org.anthracite.store` alongside the
  eight signed system apps.
- On a clean install, the store seeds the Anthracite website as its default
  catalogue source (`https://anth-panel.com/store`). If `{data}/sources.json`
  exists but has no sources, the default is added; a configured list is never
  modified.
- The website catalogue lists the official apps (the eight system apps and
  `certs`, `acme`, `proxy`, `containers`) plus third-party apps whose publishers
  registered a version, an external bundle URL, its SHA-256 and the signer key.
- The store downloads every bundle from the publisher's URL; nothing is uploaded
  to the website or to the panel.
- `demo` and `demo-sessions` do not include the store.

Existing installations: running the new `base/run.sh` adds the missing store with
`compose --add` without removing custom apps. If the store was already installed,
update its bundle from the manager; the new version seeds the source when the
list is empty.

## 0.8.0 — the App SDK

`v0.8.0` turns the public repository into an installable SDK for third-party
apps. The engine and the five edge apps are unchanged; this line is about the
authoring path.

### Rust

- `cargo install cargo-anth` provides `cargo anth`; the release also ships
  `anth`, `anth-sign` and `anth-conformance`.
- `anth app new|doctor|build|check|package|sign|verify` creates a standalone
  project in `/tmp` and ends with a signed bundle.
- Publishable crates: `anth-error`, `anth-bundle`, `anth-guest`,
  `anth-service`, `anth-client` and `cargo-anth`. `anth-bundle` owns the
  canonical signed message, WIT/file hashes, confined zip unpack and Ed25519;
  `anth-error` is the small error model extracted from the contract.

### Bun

- `bun add @anth-panel/panel @anth-panel/client` for the UI contract and
  generated clients.
- `bunx @anth-panel/cli app ...` for the same workflow, with `ANTH_BIN` and
  `cargo anth` fallbacks.
- `@anth-panel/create-app` scaffolds; `@anth-panel/bun-plugin` resolves the
  virtual `anth:client` module in `Bun.build`.
- `@anth-panel/wit` carries the versioned host catalog.

### Guides and examples

- [App SDK](sdk.md) is the quickstart and reference; `anth.toml` is the only
  config.
- `examples/sdk/rust` and `examples/sdk/bun` are generated projects.
- `anth app doctor` checks cargo, rustc, the wasm target, wasm-tools, Bun and
  the signing tools.

### Images

The four container variants are published to GHCR:

| Image | Tags |
| --- | --- |
| `ghcr.io/<owner>/anthracite-panel` | `core`, `base`, `demo` and `0.8.0-*` |
| `ghcr.io/<owner>/anthracite-demo` | `main` and `0.8.0-demo-sessions` |

The versioned tags are immutable; the moving tags point at the latest release.

### Upgrade

Back up `$state`, verify `SHA256SUMS`, replace the binaries/bundles, and restart.
`0.7.x` state is read by `0.8.0`; a downgrade is not supported after a `0.8.0`
binary writes the state. The engine contract is unchanged; the new packages are
additive.

### Known limitations

- Bun 1.4.4 does not apply a `bunfig.toml` preload plugin to build resolution;
  register `@anth-panel/bun-plugin` in `Bun.build` explicitly.
- The native-service template lands with the service SDK; the current
  templates are Rust wasm and Bun panel.
- Egress remains intent until L2; the container runtime socket is
  root-equivalent for containers.
- Publication from CI still waits for a GitHub runner; the scripts are manual.

## 0.7.0 — the edge composition

`v0.7.0` is the M6 milestone: five business apps, the sandbox L1 and the port
capability, proven on the edge composition of
[§21 §3](../design/21-next-version-apps.md#3-the-edge-composition).

### Five apps

- **`certs`** — inventory, import, validate and publish certificates; the proxy
  reads `{shared}/tls/<id>/`. Private keys are never returned by a route or WIT.
- **`acme`** — RFC 8555 issuance and renewal; HTTP-01 through the proxy,
  DNS-01 through `manual`, `rfc2136` or a `webhook`; staging is never promoted.
- **`proxy`** — HTTP/HTTPS vhosts, WebSocket, HTTP/2, whole-document apply with
  rollback, TLS reload and a lossy access log that reports its drops.
- **`containers`** — Docker-compatible lifecycle, logs, stats, images, volumes
  and exec behind the approved runtime socket; no bind mounts or privileged mode.
- **`store`** — signed catalogues, digest-pinned downloads and the manager
  handoff. The store never installs; the manager holds the operator session.

### Platform

- **Manifest v4**: `roots`, `listen`, `egress` and `{broker}`; the intent is
  shown at approval and frozen. v3 manifests are still accepted.
- **Sandbox L1**: per-service uid when the engine is root, Landlock roots,
  `CAP_NET_BIND_SERVICE` for declared privileged listeners, and the existing L0
  process hygiene. A kernel that cannot apply L1 refuses the launch.
- **Broker, audit bridge and surface exports**: services call each other through
  `{broker}`; `anth:audit/record@1.0.0` writes sanitized records; the composition
  surface lists each app's exports, so the manager finds providers by interface.
- **`[composition] allow-degraded`** (default `false`): an import with no
  provider is fatal unless the operator opts in; then the consumer degrades with
  `unresolved` and the rest boots. This is what makes removing one of the five
  apps a supported operation.
- A service that declares an egress list has it shown as **intent**; network
  isolation (L2) is not in this release.

### Removal

Removing an app leaves a working system with that section gone. The golden
matrix in `engine/api/tests/golden.rs` asserts zero fatal and the exact degraded
set for each removal. Uninstalling never deletes `{data}` or files published
under `{shared}`; cleanup is explicit.

### Upgrade

Back up `$state`, verify the release artifacts, stop the process, replace the
binaries and bundles, start again, then check `anth status` and `anth audit
--limit 20`. State from 0.6.x is read by 0.7.0; a downgrade is not supported
after a 0.7.0 binary has written the state. A release build still refuses a
system bundle signed by another key, so a key rotation is a release.

### Known limitations

- Egress is intent until L2. A compromised service is stopped by L1 roots and
  ports, not by a network policy.
- The container runtime socket is root-equivalent for containers; the approval
  says so.
- ACME production CAs, Docker and rootless Podman are manual QA rows; the
  reproducible half is the RFC 8555 fixture and the Engine API fixture.
- The release is only as strong as the key ceremony: the private key stays with
  the operator, and `scripts/release.sh` compiles only the public half in.
- There is no debug-preview release (D19).
