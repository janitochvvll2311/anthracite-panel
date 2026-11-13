# Report — v0.6.0 system composition

## Summary

Phase 7 closes M4 with a golden surface for the system apps and a removal test
for `net`. The fixture is rendered from the real `apps/<name>/manifest.toml.in`
and `apps/<name>/wit/` files, so routes, slots, CLI verbs, interfaces and risk
are not copied into a second source of truth. Removing `net` leaves the other
seven apps running, and Firewall falls back to a top-level tab in the panel.

## Integrated phases

| Phase | PR | State |
| --- | --- | --- |
| Phase 7 — Manager and M4 close | #162 | open |

The manager itself landed in #104; this report covers the composition close.

## Delivered

- `demo/composition.toml` is the source of the system app list; the test filters
  `org.anthracite.net` for the removal case instead of keeping a second list.
- `tests/fixtures/expected/system.json`: the blessed surface (routes, slots,
  CLI, risk and boot order).
- `engine/api/tests/golden.rs`: renders the real manifests with deterministic
  fixture values, copies the real WIT, approves each app with the risk of its
  service class and asserts the golden surface.
- `removing_net_leaves_the_other_system_apps`: no fatal, no degraded, seven
  apps, Firewall present with its `parent = "org.anthracite.net#anth-net"`
  reference intact.
- `panel/frontend/src/groups.test.ts`: Firewall keeps its place as a top-level
  tab, after Terminal, when Network is removed.

## Tests

- `engine/api` golden suite: **14 tests**, including the system surface and the
  `net` removal.
- `panel/frontend`: **18 tests**.
- `cargo fmt --all --check` and `clippy -D warnings` on the touched crates.
- The panel grouping fallback is unit-tested; no browser e2e is added.

## Decisions

- The system fixture renders the real manifests instead of duplicating them.
  `{{app_name}}`, `{{service_*}}` and asset hashes get deterministic values.
- The app list comes from `demo/composition.toml`. `SYSTEM_CLASSES` maps each
  app to its `ServiceClass`, and risk comes from `class.risk()`, not a second
  risk table. The real app WIT is copied from the apps, so interface drift still
  fails the golden.
- The fixture uses `allow-third-party-service` because v0.6.0 has no distribution
  key yet; a release build still refuses third-party services by default.
- Removing `net` does not rewrite the Firewall manifest. The engine keeps the
  parent reference and the panel's grouping function promotes the orphan to a
  top-level tab.

## Out of scope

- Starting real host services in CI. The golden test resolves the composition;
  the demo composition and the manual smoke remain the boot proof for real
  binaries.
- Browser e2e for the grouped tab. The grouping fallback is a unit test, and the
  existing panel QA matrix covers the iframe rendering.
- A distribution key or system tier in v0.6.0.

## Pending and risks

- The golden surface must be re-blessed (`BLESS=1`) when a system manifest,
  slot, route or CLI intentionally changes.
- A real service boot still needs the demo bundles or a host with the approved
  services; it is not part of the unit suite.
