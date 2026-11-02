# 15. Open decisions

> Part of the [design index](../../DESIGN.md).

| # | Decision | Recommendation | By |
| --- | --- | --- | --- |
| D1 | Composition scope: system-wide vs. per-user | System-wide; per-user profiles later | M1 |
| D2 | Recompose: process restart vs. per-app restart | Process restart; revisit after M3 | M3 |
| D3 | PTY shape: two `stream<u8>` vs. a `resource` with methods for resize and signals | A resource, so control messages do not become protocol bytes | M5 |
| D4 | When to ship the Pulley profile | Only if a deployment target requires it; never as default | M5 |
| D5 | Blob transfer shape | `stream<u8>`; ships with transfers in v0.6.0 | v0.6.0 |
| D6 | Revocation model: CRL, key rotation, or both | Key rotation in a release first; a CRL only if a real fleet needs it | v0.6.0 |
| D7 | Remote panel: when, and over which transport (HTTP + session, or a thin RPC) | Design-capable now, implement after M3; HTTP with the same session model | v0.6.0 |

---
