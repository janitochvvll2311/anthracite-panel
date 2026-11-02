# 11. The scaffold: what we build

> Part of the [design index](../../DESIGN.md).

Milestones are ordered so each one is demonstrable on its own.

### v0.5.0 — the platform (M0–M3)

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
- Toolchain gate: `wit-bindgen` builds a component and wasmtime instantiates it,
  offline, with the pinned toolchain.
- Async probe, recorded and not shipped: build a component with a `stream` under
  WASI 0.3 and run it on the pinned wasmtime. Its result is what M5 designs the
  PTY against.

**Done when:** `anth-engine --config …` boots with an empty composition, `anth
doctor` (from the SDK) is green against it, the edge check fails on a seeded
violation, and contract tests cover config and error shapes.

**M1 — Composition**

- Registry: install, list, verify, replace, remove; zip path safety, signatures,
  hashes, WIT validation. Creating is `POST`, replacing is `PUT`.
- Manifest v2 parser, interface catalog, risk computation.
- Grant store and `anth app approve` with the `--accept-*` levels.
- Resolver with the eight steps of §8.2, plus typed errors for cycle, duplicate
  export, unresolved import and blocked app.
- The composition view over HTTP: what is installed, what is composed, which app
  contributes which slot, and the CLI surface per app.
- `anth compose` and the three failure phases of §8.5.
- `/api/engine/keys` and `anth key`, so a third-party signer can be trusted.
- The acknowledgment pattern: breaking actions return `409` with the tokens they
  need (§6.4).
- `anth status` over health plus composition.

**Done when:** fixture compositions produce golden surfaces, and every failure
mode in `tests/fixtures/` yields its specific typed error.

**M2 — First app**

- Component runtime with fuel, memory and wall backstop.
- World-vs-grant validation (§7.12) and the three refusal tests.
- Host interfaces `anth:env/*` and `anth:store/kv`.
- Routes to `interface#function`, with the value mapping of §7.13 and the error
  mapping of §7.11.
- `sdk/rust/guest` plus the codegen that turns WIT into CLI commands and clients.
- `apps/hello`: one exported interface, one imported interface, one route, one
  slot contribution, CLI verb generated from WIT.

**Done when:** `hello` answers over HTTP and CLI, and a deliberately broken
component (extra import, drifted type, fuel exhaustion, denial) yields the right
typed error instead of a crash.

**M3 — Panels**

- `sdk/rust/panel` and `sdk/ts/panel`: the panel contract, slot registry, iframe
  bridge and scoped `postMessage` RPC.
- `panel/backend`: links the SDK, mounts the engine router, wires the CLI, embeds
  `panel/web/dist` and fails the release build without it.
- `panel/web`: a Bun project and a real shell — sidebar, tabbed views, an action
  button and detachable floating windows — rendering the three standard slots.
- Asset serving from bundles, hash-pinned, CSP-clean.
- A **second, minimal panel** written against the SDK only. It implements
  `panel.view` at minimum; that is what proves the vocabulary belongs to the
  contract and not to our panel.

**Done when:** `hello` renders in both panels, in a tab that can be detached, and
neither was compiled against it. **That is the v0.5.0 acid test.**

### v0.6.0 — the proof (M4–M5)

**M4 — System apps**

`fs`, `terminal`, `proc`, `net`, `storage`, `accounts`, `manager`. Each is a
bundle with a manifest; each is removable; none requires an engine edit. Heavy
compute (archives, previews, hashing) stays in wasm.

**Done when:** a composition of system apps reproduces the v0.4.0 feature
baseline, and removing `net` leaves a working system with that section gone — not
a broken panel.

**M5 — Third party**

- Streams, designed against the terminal rather than a toy. Backpressure,
  cancellation, half-close and error-after-partial-data each need a conformance
  test or the feature does not ship.
- Typed events (§7.8) on top of those streams, as `subscribe() -> stream<event>`.
- Quotas per stream (bytes/s and total) enforced from the budget.
- Conformance kit published; `anth-conformance` runs against a bundle offline.
- Audit report, approval/diff UI in the panel's settings slot.
- Sandbox hardening for services; decision on shipping the Pulley profile.

**Done when:** an app that lives outside this repository installs, composes,
renders and answers — the acid test, executed by someone who is not us.

### The platform's definition of done

1. `anth-engine` with no apps is a valid, healthy system.
2. Every product feature **except the panel** is a bundle under `apps/`, removable
   from the composition; the panel is an application built on the SDK.
3. No engine source file mentions a feature name, a slot, or a pixel.
4. A new app requires zero engine changes.
5. A new panel requires zero engine changes — only the SDK.
6. The conformance kit fails loudly on a bundle that lies.

---
