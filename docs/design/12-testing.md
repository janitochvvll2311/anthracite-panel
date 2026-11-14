# 12. Testing the platform

> Part of the [design index](../../DESIGN.md).

The platform is tested by *contract*, not by feature coverage.

- **Contract tests** for manifests, interfaces, grants, composition and error
  shapes, including property tests over path safety, manifest parsing and
  resolver determinism.
- **World-vs-grant tests**: extra import, missing export, drifted type, wrong
  version — each must refuse with its own error and no instantiation.
- **Golden compositions**: fixtures asserting a full surface (routes, slots, order,
  CLI verbs) and, separately, each failure class — **fatal**, **blocked**,
  **degraded** — because they have different causes and different fixes.
- **Signature and tamper tests**: a wasm swapped after signing, a WIT file edited
  after approval, an unsigned bundle in a release build, a bundle whose manifest
  names another id. Each must refuse, and the refusal must name the file.
- **Component conformance guests**: small components for each failure path —
  trap, fuel exhaustion, denial, oversized payload.
- **Error envelope and acknowledgment**: every code maps to its status and exit
  code; a breaking action without `X-Anth-Acknowledge` returns `409` with the
  tokens, with it proceeds, and the audit entry names the dependents.
- **CLI and API parity**: every `[[cli]]` verb resolves to a route bound to the
  same function, and the generated clients compile against the same WIT.
- **Doctor offline**: a broken composition is diagnosed with the engine stopped.
- **Golden HTTP traces**: the surface's responses, including degraded 503s.
- **Panel tests**: the official panel and the minimal custom panel both render a
  slot contributed by an app neither was compiled against. The custom panel is
  the test that keeps the SDK honest.
- **Edge tests**: `cargo metadata` asserts the forbidden edges of [§10.1](10-workspace.md#101-the-edges-we-forbid); a seeded
  violation must fail the build.
- **The conformance kit** is the same suite a third party runs against their
  bundle. If our tests and their tests are different, the platform is not real.
- **The acid test is a script**, not a demo: `scripts/acid-test.sh` builds an app
  that lives outside `apps/`, signs it with its own key and drives a real panel
  through install, approval, composition, asset serving and a route call.

Two rules for the suite:

- **Assert outputs, never durations.** Fuel is per-instruction and portable
  between backends; the clock is not.
- **Golden tests use `--json`.** A table whose widths depend on the terminal is a
  test that passes locally and fails in CI.
- **The backend is not a contract.** Nothing asserts Cranelift-specific
  behaviour. If the Pulley profile ever ships, the same suite must pass on it.

e2e (browser) tests exercise the official panel and one contributed slot, not
every feature.

---
