# Certificates

The certificate app is the inventory and publish service of the edge
composition. It lists certificates, validates an imported chain and key, and
publishes the material the proxy reads. It is not a CA and never copies a
private key out of a system store.

Design: [§23](../design/23-certificate-manager.md). Decision: [ADR 0006](../decisions/0006-where-does-an-issued-certificate-key-live-when-the-certificate-s.md).

## Approval

| Field | Value |
| --- | --- |
| App id | `org.anthracite.certs` |
| Service class | `admin` |
| Roots | `/etc/ssl/certs:ro` and an operator-declared scan root |
| Interface | `anthracite:certs/store@1.0.0` |

At approval the panel shows the scan roots and the fact that publishing writes
into `{shared}/tls`. A certificate published there is readable by any service
whose grant includes `{shared}`: that is the service trust domain, not a
per-file ACL.

## Behaviour

- Inventory: `/etc/ssl/certs`, an operator-declared Let's Encrypt live directory
  and the managed store. Status is `valid`, `expiring` (30/14/7 days), `expired`
  or `invalid`.
- Import: a PEM chain and key are checked against each other before anything is
  written. The key is stored at `{data}/certs/<id>/` (`0600`).
- Publish: `{shared}/tls/<id>/fullchain.pem`, `privkey.pem` and `meta.json`, all
  `0600`, written atomically.
- `auto-renew` is only accepted for `source = acme`; an imported certificate is
  never renewed behind the operator's back.
- Removing the app keeps published files and imported state. The proxy keeps
  serving until the certificate expires; the panel shows the missing provider.

## Routes and CLI

| Route | CLI | Function |
| --- | --- | --- |
| `GET /certs` | `anth list` | inventory |
| `GET /certificate` | `anth show` | one certificate |
| `POST /certs` | `anth import` | import chain and key |
| `POST /certificate/validate` | `anth validate` | check a pair |
| `POST /certificate/publish` | — | publish to `{shared}/tls` |
| `POST /certificate/auto-renew` | — | set the renewal flag |
| `DELETE /certificate` | — | remove a managed certificate |

The private key is never returned by WIT, a route, the panel or a log.

## Tests

`apps/certs/backend` covers PEM parsing, key match, expiry boundaries, atomic
replace and permissions. The edge removal matrix in
`engine/api/tests/golden.rs` asserts that removing this app leaves `acme`
degraded with `unresolved` and the rest running.
