# Report — Firewall app

## Summary

Firewall is now its own app, grouped under Network as a sub-tab. The platform gained one-level view grouping, the panel renders nested sub-tabs, and the new `org.anthracite.firewall` bundle owns a managed `inet anthracite` nftables table with input/output chains, rules, custom rules, flush and default policies.

## Integrated phases

| Phase | PR | State | Merge |
|-------|----|-------|-------|
| Slot grouping contract and SDK | #152 | merged | `64679d4` |
| Panel nested view tabs | #153 | merged | `299b4bb` |
| Firewall app and Network sub-tab | #154 | merged | `cb96273` |

## Delivered features

- `[[slot]]` accepts `parent = "<app-id>#<slot-tag>"` and `tab`, with manifest validation and surface/SDK propagation.
- The panel groups child views under a parent tab, renders sub-tabs, keeps each child frame isolated, closes the group together and falls back to top-level when the parent is missing or hidden.
- `org.anthracite.firewall` with `anthracite:firewall@1.0.0`: snapshot, add/update/delete rule, flush and set policy.
- `firewall-service` owns `inet anthracite`, uses `nft` by argv, parses the JSON table, keeps custom rules read-only and gates mutations with `--firewall-control`.
- Firewall view with chain policies, managed/custom rules, add/edit/delete, flush and destructive confirmations in the sandboxed iframe.
- Deterministic in-memory firewall fixture and bundle for the demo; Network's slot is labelled `Interfaces` and the Firewall slot names Network as parent.
- Demo composition, README, baseline, roadmap and phase plan updated.

## Review findings addressed

- Slot contract: Rust SDK grouping fields, parent tag reference, character-based tab limit, surface propagation test.
- Panel: Float acts on the active child, no hidden parent pre-mount, stale mobile grid rule removed, comments aligned with the tag reference.
- Firewall: quoted nft comments, query-string handles, real status selector and a native `<dialog>` confirmation inside the iframe.

## Tests

- `apps/firewall/backend`: **10 tests**, `clippy -D warnings` and `fmt --check` clean.
- `demo/emulator`: **47 tests**, clippy and fmt clean.
- `panel/frontend`: **14 tests**, build clean.
- Engine contract/API and both SDKs: contract, surface and grouping tests clean.
- Demo e2e through the engine: eight apps boot, the composition exposes `tab=Interfaces` and `parent=org.anthracite.net#anth-net`, and Firewall snapshot/add/policy/flush/delete work against the fixture.
- Real service smoke: `/health` responds; `snapshot` returns `denied` without `CAP_NET_ADMIN`, as expected in this environment.

## Recorded decisions

- Firewall is a separate app grouped visually under Network; there are no cross-app calls or shared grants.
- Grouping is one level and falls back to a top-level tab when the parent is missing, hidden or nested.
- The demo simulates firewall rules in memory and never applies them to the host.
- Real firewall validation is manual in a container with `--cap-add=NET_ADMIN`.

## Out of scope

- NAT, sets/maps, arbitrary expressions, ipsets or tables other than `inet anthracite`.
- Multi-user permissions and the manager/audit UI.
- Automated real firewall validation in CI.

## Pending and risks

- Run the manual `nft` validation with `CAP_NET_ADMIN`.
- A `deny` policy can cut management access; the UI and README warn about it.
- Grouped floating keeps the group docked and floats the active view only; a full group floating window remains a possible follow-up.
