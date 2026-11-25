# 11. The scaffold: what we build

> Part of the [design index](../../DESIGN.md).

Milestones are ordered so each one is demonstrable on its own.

### v0.5.0 — the platform (M0–M3)

> **Status:** implemented. The auth boundary (first-boot credentials, TCP
> sessions, panel sessions, origin checks and security headers) landed after M3
> as the v0.5.0 auth slice; M1–M3 are composed and exercised by the two panels.

**M0 — Kernel (no features)**

- The engine, SDK and panel workspaces, and the Bun ones, with the shared target
  directory. `apps/` arrives in M2 with `hello`.
- `engine/contract`, config, logging, one router over UDS + TCP.
- `/health`, `/api/health`, `/api/engine/version`, `/api/engine/composition`.
- Auth once in the router: peer credentials on the socket, sessions over TCP,
  origin checks and CSP.
- Supervisor skeleton with the instance registry, and orderly shutdown.
- The `Runtime` trait with a working fake backend, so engine tests never need
  wasmtime and nothing above the trait learns its semantics.
- The CI edge check: `cargo metadata` fails on a forbidden dependency.
- Toolchain gate, already proven in the spike: a component built for
  `wasm32-unknown-unknown`, embedded and wrapped with `wasm-tools`, instantiates
  under wasmtime with a typed host import and a fuel trap.
- Async probe, already done and recorded: `stream<u8>` survives the whole
  pipeline and a guest reads it with `block_on` inside a synchronous export.
  `async func` also works, and is deliberately not used ([§7.15](07-app.md#715-streams)).

**Done when:** `anth-engine --config …` boots with an empty composition, `anth
doctor` (from the SDK) is green against it, the edge check fails on a seeded
violation, and contract tests cover config and error shapes.

**M1 — Composition**

- Registry: install, list, verify, replace, remove; zip path safety, signatures,
  hashes, WIT validation. Creating is `POST`, replacing is `PUT`.
- Manifest v2 parser, interface catalog, risk computation.
- Grant store and `anth app approve` with the `--accept-*` levels.
- Resolver with the eight steps of [§8.2](08-composition.md#82-resolution), plus typed errors for cycle, duplicate
  export, unresolved import and blocked app.
- The composition view over HTTP: what is installed, what is composed, which app
  contributes which slot, and the CLI surface per app.
- `anth compose` and the three failure phases of [§8.5](08-composition.md#85-missing-and-failed).
- `/api/engine/keys` and `anth key`, so a third-party signer can be trusted.
- The acknowledgment pattern: breaking actions return `409` with the tokens they
  need ([§6.4](06-engine.md#64-http-surface)).
- `anth status` over health plus composition.

**Done when:** fixture compositions produce golden surfaces, and every failure
mode in `tests/fixtures/` yields its specific typed error.

**M2 — First app**

- Component runtime with fuel, memory and wall backstop.
- World-vs-grant validation ([§7.12](07-app.md#712-enforcement-the-world-must-match-the-grant)) and the three refusal tests.
- Host interfaces `anth:env/*` and `anth:store/kv`.
- Routes to `interface#function`, with the value mapping of [§7.13](07-app.md#713-http-and-wit-one-mapping) and the error
  mapping of [§7.11](07-app.md#711-errors).
- `sdk/rust/guest` plus the codegen that turns WIT into CLI commands and clients,
  and the build wrapper that runs `embed` + `component new`.
- `apps/hello`: one exported interface, one imported interface, one route, one
  slot contribution, CLI verb generated from WIT.

**Done when:** `hello` answers over HTTP and CLI, and a deliberately broken
component (extra import, drifted type, fuel exhaustion, denial) yields the right
typed error instead of a crash.

**M3 — Panels**

- `sdk/rust/panel` and `sdk/ts/panel`: the panel contract, slot registry, iframe
  bridge and scoped `postMessage` RPC.
- `panel/backend`: links the SDK, mounts the engine router, wires the CLI, embeds
  `panel/frontend/dist` and fails the release build without it.
- `panel/frontend`: a Bun project and a real shell — sidebar, tabbed views, an action
  button and detachable floating windows — rendering the three standard slots.
- Asset serving from bundles, hash-pinned, CSP-clean.
- A **second, minimal panel** written against the SDK only. It implements
  `panel.view` at minimum; that is what proves the vocabulary belongs to the
  contract and not to our panel.

**Done when:** `hello` renders in both panels, in a tab that can be detached, and
neither was compiled against it. **That is the v0.5.0 acid test.**

### v0.6.0 — the proof (M4–M5)

> **Plan:** the phased, PR-sized breakdown is [§18](18-v0.6-plan.md).

**M4 — System apps**

`fs`, `terminal`, `proc`, `net`, `storage`, `accounts`, `firewall`, `manager`.
Each is a bundle with a manifest; each is removable; none requires an engine
edit. Heavy compute (archives, previews, hashing) stays in wasm.

`net` owns the interface inventory. `firewall` is its own app, grouped under
Network as a sub-tab: one managed `inet anthracite` nftables table with
input/output chains, rule CRUD, flush and default policy. The grouping is visual;
the firewall remains a separate bundle and is not an engine feature.

**Done when:** a composition of system apps works on the v0.5.0 contract, the
v0.4.0 inventory is covered as a scope reference, and removing `net` leaves a
working system with that section gone — not a broken panel.

**Status (v0.6.0):** delivered. The golden suite boots the real system
composition and removes `net` without breaking the rest; see
[`docs/features/v0.6-close/report.md`](../features/v0.6-close/report.md).

The **demo** belongs here too, and as a separate instance rather than a flag: a
`config.demo.toml` with its own `state` that composes `compositions/demo.toml`, whose
apps export the real interfaces with generated, deterministic data. The engine does
not know it is a demo; a demo provider is a normal app. One composition lives in one
state directory ([§8.1](08-composition.md)), so switching between real and demo means
running a second instance, not changing a mode.

Per-browser isolated state is the part that still waits on per-user profiles
([§15](15-open-decisions.md)); until that decision lands, a demo instance shares
state across browsers.

**M5 — Third party**

- Streams, designed against the terminal rather than a toy. Backpressure,
  cancellation, half-close and error-after-partial-data each need a conformance
  test or the feature does not ship.
- Typed events ([§7.8](07-app.md#78-events)) on top of those streams, as `subscribe() -> stream<event>`.
- Quotas per stream (bytes/s and total) enforced from the budget.
- Conformance kit published; `anth-conformance` runs against a bundle offline.
- Audit report, approval/diff UI in the panel's settings slot.
- Sandbox hardening for services; decision on shipping the Pulley profile.

**Done when:** an app that lives outside this repository installs, composes,
renders and answers — the acid test, executed by someone who is not us. The
mechanism is exercised in CI by `scripts/acid-test.sh` against
`tests/acid/app`, which is outside `apps/` and outside the apps workspace; the
"not us" half is a release-time check, not something a repository can prove.

**Status (v0.6.0):** delivered. The CI flow passes; the external app is built with
plain `cargo`, signed with its own key, installed with `require-signature = true`,
approved, composed, served and answered by a real panel. The milestone close,
including the operational items that remain, is
[`docs/features/v0.6-close/report.md`](../features/v0.6-close/report.md).

### v0.7.0 — the operated edge (M6)

> **Plan:** the five business apps and their prerequisites are
> [§21](21-next-version-apps.md), phased in [§29](29-v0.7-plan.md). Status:
> **accepted**; decisions D9–D20 are binding in [§16](16-decision-log.md).

v0.6.0 proves the platform can run system apps and third-party apps. v0.7.0 uses
that proof to run an edge: TLS material and certificates, one ingress in front of
the workloads, containers behind it, and a store that installs apps without
giving an app the operator's authority.

- [`certs`](23-certificate-manager.md) owns certificate and key custody; private
  keys are never returned through WIT, HTTP or the panel, and the manual import
  is an explicit write.
- [`acme`](24-acme-client.md) issues and renews against any RFC 8555 CA, with
  HTTP-01 through the ingress and DNS-01 through a solver.
- [`proxy`](25-reverse-proxy.md) is the data-plane ingress: virtual hosts, path
  routes, TLS, WebSockets and atomic apply. It is a service, not an engine
  feature.
- [`containers`](26-container-manager.md) manages a Docker-compatible runtime,
  with logs, stats and exec behind the same approval.
- [`store`](27-app-store.md) is a provider-agnostic catalogue with signed indexes
  and digest-pinned bundles. It cannot install or approve; the panel manager does
  that with the operator's session.

The milestone adds generic platform work first: a service-to-service broker, a
manifest that declares listeners and egress, secret custody, surface exports and
a service audit bridge ([§22](22-platform-prerequisites.md)).

**Done when:** a new host can issue or import a certificate, serve two hosts
through the proxy, operate a container without SSH, and install a signed bundle
through the manager with an explicit approval; and removing any of the five
leaves a working system with only the expected degraded section — asserted by
the golden edge composition.

**Status (v0.7.0):** shipped and tagged at `1f09a3a`. The five apps, the platform
prerequisites and the sandbox shipped through
[`docs/design/29-v0.7-plan.md`](29-v0.7-plan.md); the removal matrix is
`engine/api/tests/golden.rs`, the release gate and manual QA are
[`docs/guide/release-notes.md`](../guide/release-notes.md) and
[`docs/guide/edge-qa.md`](../guide/edge-qa.md), and the close report is
[`docs/features/v0-7-0/report.md`](../features/v0-7-0/report.md).

### v0.8.0 — the third-party SDK

> **Contract:** [§31](31-sdk-distribution.md). Status: **in progress**; the
> packages are not published yet.

v0.8.0 turns the public repository into an installable SDK. One Rust CLI
(`cargo anth` / `anth`) and one npm entry point (`bunx @anth-panel/cli`) create,
build, package, sign and verify apps outside this repository. The public crates
are `anth-bundle`, `anth-guest`, `anth-service`, `anth-client` and `cargo-anth`;
the npm packages live under `@anth-panel`. The engine and its authority stay
internal.

**Done when:** `cargo install cargo-anth` and `bunx @anth-panel/cli` work in a
clean environment, the templates build/sign/verify without the monorepo, and the
compatibility matrix is documented.

### The platform's definition of done

1. `anth-engine` with no apps is a valid, healthy system.
2. Every product feature **except the panel** is a bundle under `apps/`, removable
   from the composition; the panel is an application built on the SDK.
3. No engine source file mentions a feature name, a slot, or a pixel.
4. A new app requires zero engine changes.
5. A new panel requires zero engine changes — only the SDK.
6. The conformance kit fails loudly on a bundle that lies.

---
