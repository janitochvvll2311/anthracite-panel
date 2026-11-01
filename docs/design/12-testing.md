# 12. Testing the platform

> Part of the [design index](../../DESIGN.md).

The platform is tested by *contract*, not by feature coverage.

- **Contract tests** for manifests, interfaces, grants, composition and error
  shapes, including property tests over path safety, manifest parsing and
  resolver determinism.
- **World-vs-grant tests**: extra import, missing export, drifted type, wrong
  version — each must refuse with its own error and no instantiation.
- **Golden compositions**: fixtures asserting a full surface (routes, slots,
  order, degraded set) and a specific error for the invalid ones.
- **Component conformance guests**: small components for each failure path —
  trap, fuel exhaustion, denial, oversized payload.
- **Golden HTTP traces**: the surface's responses, including degraded 503s.
- **Panel tests**: the official panel and the minimal custom panel both render a
  slot contributed by an app neither was compiled against. The custom panel is
  the test that keeps the SDK honest.
- **Edge tests**: `cargo metadata` asserts the forbidden edges of §10.1; a seeded
  violation must fail the build.
- **The conformance kit** is the same suite a third party runs against their
  bundle. If our tests and their tests are different, the platform is not real.

Two rules for the suite:

- **Assert outputs, never durations.** Fuel is per-instruction and portable
  between backends; the clock is not.
- **The backend is not a contract.** Nothing asserts Cranelift-specific
  behaviour. If the Pulley profile ever ships, the same suite must pass on it.

e2e (browser) tests exercise the official panel and one contributed slot, not
every feature.

---
