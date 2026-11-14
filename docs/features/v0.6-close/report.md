# Report — v0.6.0 close

## Summary

Plan [§18](../../design/18-v0.6-plan.md) ran phases 0–9, each as its own PR into
`v0.6.0`. The three definitions of done are met: the system apps work on the
v0.5.0 contract, an app from outside this repository installs, composes, serves
and answers, and the conformance kit refuses a bundle that lies. What is left is
operational, not code.

## Phases and evidence

| Phase | Result | Evidence |
| --- | --- | --- |
| 0 — Baseline and ADRs | accepted | [§19](../../design/19-v0.6-baseline.md), [§16](../../design/16-decision-log.md) |
| 1 — Service app contract | v3 manifests, WIT RPC | [§19](../../design/19-v0.6-baseline.md) |
| 2 — Host emulator and demo | removable demo service | `demo/`, [v0.6-system-composition](../v0.6-system-composition/report.md) |
| 3 — System app UIs | slots and modals against the emulator | [ui-fab-modals](../ui-fab-modals/report.md), [v0.6-system-composition](../v0.6-system-composition/report.md) |
| 4 — Real `fs` | confined host filesystem service | [v0.6-system-composition](../v0.6-system-composition/report.md) |
| 5 — Real terminal | PTY service over the private socket | [v0.6-system-composition](../v0.6-system-composition/report.md) |
| 6 — proc, net, storage, accounts | services plus the firewall app | [firewall](../firewall/report.md), [real-accounts](../real-accounts/report.md) |
| 7 — Manager and M4 close | approval, risk and diff | [authority-diff](../authority-diff/report.md), [audit-log](../audit-log/report.md) |
| 8 — Streams, events, quotas | M5 stream contract | `docs/design/20-streams.md` |
| 9 — Conformance, third party, hardening | kit, sandbox L0, tier, signing, acid test | [conformance](../conformance/report.md), [fetch-wit](../fetch-wit/report.md), [service-hardening](../service-hardening/report.md), [system-tier](../system-tier/report.md), [bundle-signer](../bundle-signer/report.md), [namespace-tier](../namespace-tier/report.md), [acid-test](../acid-test/report.md), [release-ceremony](../release-ceremony/report.md) |

Cross-cutting work that does not belong to a phase has its own report:
[ci-v0.6](../ci-v0.6/report.md) and [cli-api-parity](../cli-api-parity/report.md).

## Delivered in the close

- `anth operator`, `anth restart` and `compose --now` closed the engine command
  gaps; the only remaining CLI/API difference is `anth login/logout`, which is a
  transport question rather than a missing route.
- `scripts/release.sh` turns the distribution key into a release: keyed binaries,
  signed bundles and a boot check that each trusts the same fingerprint.
- A status line in [§18](../../design/18-v0.6-plan.md), [§11](../../design/11-roadmap.md)
  and `AGENTS.md` says where the project is, so the next agent does not have to
  reconstruct it from the reports.

## Operational pending

- **Green CI.** Every GitHub job fails in about two seconds with `runner_id: 0`
  since 2026-11-09; the workflows are correct but no runner picks the jobs up.
  The v0.5.0 release follow-up of [§17 §0](../../design/17-deferred.md) is the
  same blocker.
- **The release key pair.** The ceremony is implemented and exercised with a
  throwaway key; producing the first real release needs the operator to hold the
  private half and run `scripts/release.sh`.
- **Real-host validation of the native services.** PTY, mounts and
  `CAP_NET_ADMIN` have unit and fixture coverage but have not been exercised on a
  deployment host here.
- **Browser QA.** The panel's manual pass is a human task; the automated checks
  cover assets, composition and the routes.

## Deferred beyond v0.6.0

- Sandbox layers L1–L4 (namespaces, Landlock, capabilities/seccomp, cgroups) are
  proposed in [§28](../../design/28-service-sandbox.md); L0 shipped.
- Pulley is not shipped; Cranelift remains the only backend, by decision.
- `anth login/logout` over HTTP and generated clients for the system apps are
  deliberate non-goals, documented in the parity report.
- A registry, a version solver and the app store ([§27](../../design/27-app-store.md))
  are the v0.7 conversation.
