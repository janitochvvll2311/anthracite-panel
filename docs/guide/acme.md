# ACME

The ACME app issues and renews certificates against an RFC 8555 directory. It
implements HTTP-01 and DNS-01, keeps account keys in `{data}`, and stores issued
material through the certificate app over the broker.

Design: [§24](../design/24-acme-client.md). Decisions: [ADR 0004](../decisions/0004-how-does-the-acme-client-allowlist-ca-directories-and-opt-into-a.md),
[0005](../decisions/0005-how-does-a-manual-dns-01-order-get-its-confirmation.md),
[0006](../decisions/0006-where-does-an-issued-certificate-key-live-when-the-certificate-s.md),
[0007](../decisions/0007-how-does-the-panel-know-which-ca-directories-are-staging.md),
[0008](../decisions/0008-how-is-the-acme-gate-automated-when-ci-cannot-download-or-run-pe.md).

## Approval

| Field | Value |
| --- | --- |
| App id | `org.anthracite.acme` |
| Service class | `socket` |
| Egress | `any` (the CA is not known until the allowlist is read) |
| Interface | `anthracite:acme/client@1.0.0` |
| Imports | `anthracite:certs/store@1.0.0` |

The directory URLs are not argv: they come from `{data}/config.json`, an
allowlist of `url`, `label`, `staging` and `allow-private`. `create-account`
accepts a listed URL only. A non-HTTPS URL and loopback, link-local, private or
cloud-metadata targets are refused unless that entry sets `allow-private`; an
internal CA root is trusted only through the optional `ca-bundle` PEM.

## Behaviour

- HTTP-01 writes the key authorization to `{shared}/acme/http-01/<token>`; the
  proxy serves it for hosts with `acme = true`. The client fetches the challenge
  URL itself before telling the CA it is ready.
- DNS-01 solvers: `manual` (the operator confirms the TXT record), `rfc2136` and
  `webhook`. A manual order waits up to an hour and emits `challenge-ready`.
- Renewal reads `auto-renew` certificates from the certificate store and renews
  at about two thirds of lifetime or when ARI says so, with jittered backoff.
- A staging order is never promoted. Account keys and EAB secrets stay at
  `{data}/accounts/<id>/` (`0600`) and never appear in a route or log.
- If the certificate app is absent, an issued order becomes `issued-unstored`:
  the material stays in `{data}` with a warning instead of being dropped.
- Removing the app keeps existing certificates working; no renewals run.

## Routes and CLI

| Route | CLI | Function |
| --- | --- | --- |
| `GET /accounts` | `anth accounts` | list accounts |
| `POST /accounts` | `anth account` | create an account |
| `GET /orders` | `anth orders` | order timeline |
| `POST /orders` | `anth issue` | issue a certificate |
| `POST /order/confirm` | `anth confirm` | release a manual DNS-01 order |
| `POST /order/renew` | `anth renew` | renew now |
| `POST /order/revoke` | `anth revoke` | revoke with a reason |
| `GET /order/events` | — | stream timeline events |

## Tests

`apps/acme/backend/tests/loopback.rs` is the always-on RFC 8555 fixture (JWS,
HTTP-01, CSR). `tests/pebble.rs` is the real-CA gate and runs with
`scripts/acme-pebble.sh`; Let's Encrypt staging is a manual QA row.
