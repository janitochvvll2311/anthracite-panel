# 13. Migration notes (reference only)

> Part of the [design index](../../DESIGN.md).

Nothing is ported wholesale. `v0.4.0-freeze` is a library of decisions:

| Keep | Drop |
| --- | --- |
| one HTTP dialect, UDS + TCP | feature modules inside `anth-panel` |
| frozen grants, risk, approval levels | `AppService` doing install *and* serve |
| hash-pinned assets, signed bundles | v2/v3 dual manifest paths |
| argv-based `ip`/`nft`/`sfdisk` with `pid+start_ticks` guards | router that enumerates feature endpoints |
| `host.info` as a snapshot | a panel that hard-codes sections |
| route → `interface#function` dispatch | the home-grown type language and flat ABI 4 |

The v0.4.0 features are re-implemented as apps in v0.6.0, from their behaviour,
not by copying their code.

### Safety invariants that survive the port

The layout, the component structure and the code do not carry over. These
behaviours do, and they are what the v0.6.0 apps are judged against:

| Area | Invariant |
| --- | --- |
| Auth | peer credentials on the socket, sessions over TCP, same-origin and CSP in one boundary |
| Files | freedesktop trash semantics; upload conflict resolution; no path escapes |
| Terminal | owner/seq/idle limits per session |
| Processes | signals guarded by `pid + start_ticks`, never `pid` alone |
| Network | `ip` by argv, never a shell; up/down guarded by policy |
| Firewall | `nft` by argv against a managed table |
| Storage | `sfdisk`/`mkfs` by argv, explicit device allowlist |
| Accounts | `chpasswd` on stdin; `allow_privileged_groups` respected |

Anything not in this table is presentation, and presentation is free to change.

---
