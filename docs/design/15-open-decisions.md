# 15. Open decisions

> Part of the [design index](../../DESIGN.md).

| # | Decision | Recommendation | By |
| --- | --- | --- | --- |
| D1 | Composition scope: system-wide vs. per-user | System-wide; per-user profiles later. The schema already has `[[profile]]`, so this is a scoping decision, not a format change | M1 |
| D2 | Recompose: process restart vs. per-app restart | Process restart; revisit after M3 | M3 |
| D3 | PTY shape: two `stream<u8>` vs. a `resource` with methods for resize and signals | A resource, so control messages do not become protocol bytes | M5 |
| D4 | When to ship the Pulley profile | Only if a deployment target requires it; never as default | M5 |
| D5 | Blob transfer shape | `stream<u8>`; ships with transfers in v0.6.0 | v0.6.0 |
| D6 | Revocation model: CRL, key rotation, or both | Key rotation in a release first; a CRL only if a real fleet needs it | v0.6.0 |
| D7 | Remote panel: when, and over which transport (HTTP + session, or a thin RPC) | Design-capable now, implement after M3; HTTP with the same session model | v0.6.0 |
| D8 | Multiple operators, and what each may do | Single operator in v0.5.0; roles when a real deployment asks | v0.6.0 |
| D9 | How a service calls another service | A private `{broker}` socket with generated clients, authorized by the grant ([§22 §1](22-platform-prerequisites.md#1-service-call-broker)) | v0.7.0 |
| D10 | How a service declares listeners and egress | Manifest v4 optional `listen`/`egress`, shown at approval and frozen; v3 stays accepted ([§22 §2](22-platform-prerequisites.md#2-listener-and-egress-intent-in-the-manifest)) | v0.7.0 |
| D11 | Where service secrets live | `{data}` at `0600`, write-only from the panel, never in WIT output, routes or logs ([§22 §3](22-platform-prerequisites.md#3-secret-custody)) | v0.7.0 |
| D12 | How proxy configuration is edited | Whole-document `apply` with a version and last-good rollback ([§25 §3](25-reverse-proxy.md#3-configuration-model)) | v0.7.0 |
| D13 | Which ports the proxy binds by default | `:8080`/`:8443`; production via socket activation or `CAP_NET_BIND_SERVICE` ([§25 §7](25-reverse-proxy.md#7-privileged-ports)) | v0.7.0 |
| D14 | How DNS-01 providers plug into ACME | `manual`, `rfc2136` and `webhook` inside the ACME service; vendor adapters additive ([§24 §2](24-acme-client.md#2-authority-and-custody)) | v0.7.0 |
| D15 | Which container runtimes are supported | Docker-compatible API only (Docker and Podman); no pluggable runtime interface ([§26 §5](26-container-manager.md#5-runtime-compatibility)) | v0.7.0 |
| D16 | Who may install from the store | The panel manager with the operator session; the store only catalogues and downloads ([§27 §2](27-app-store.md#2-the-authority-boundary)) | v0.7.0 |
| D17 | How many store providers a composition has | One official aggregator; sources are adapters; optional-interface negotiation post-v0.7.0 ([§27 §6](27-app-store.md#6-provider-agnostic-sources)) | v0.7.0 |
| D18 | How a store index is trusted | Detached Ed25519 over the exact served bytes, plus bundle digest and engine signature ([§27 §5](27-app-store.md#5-index-format-and-trust)) | v0.7.0 |
| D19 | What a v0.7.0 release requires | Distribution key, system tier and service sandbox; otherwise a debug preview ([§22 §5](22-platform-prerequisites.md#5-system-tier-and-service-sandbox)) | v0.7.0 |
| D20 | How service actions reach the engine audit | `anth:audit/record@1.0.0` through the broker with sanitized detail ([§22 §7](22-platform-prerequisites.md#7-a-service-audit-bridge)) | v0.7.0 |

---
