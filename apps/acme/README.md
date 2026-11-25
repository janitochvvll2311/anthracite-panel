# ACME

An RFC 8555 client: accounts, orders, HTTP-01 through the ingress, DNS-01
through a manual, RFC 2136 or webhook solver, renewal and revocation. Bundle
`org.anthracite.acme`, package `anthracite:acme@1.0.0`, service class `socket`,
one `panel.view`, one FAB and one `panel.modal`.

## Authority

```toml
[service]
class = "socket"
argv = ["--socket", "{socket}", "--data", "{data}", "--shared", "{shared}",
        "--broker", "{broker}"]
egress = ["any"]

[[import]]
interface = "anthracite:certs/store@1.0.0"
```

- `egress = ["any"]` is the CA on 443; the destinations are the configured
  directory allowlist, not arbitrary URLs. Every request resolves the host and
  refuses loopback, link-local, unique-local, private and metadata addresses
  unless the entry sets `allow-private` (decision 0004).
- The HTTP-01 token directory is `{shared}/acme/http-01` (derived from the single
  `{shared}`). The proxy serves it for hosts with `acme = true`; any other
  ingress can read the same directory.
- The certificate store is reached through the broker only
  (`anthracite:certs/store#store-issued`). This process never publishes TLS
  material.
- Account keys and certificate keys never leave `{data}`; no route returns,
  logs or audits them.

## Configuration

`{data}/config.json` (0600) is written with Let's Encrypt production and
staging the first time the service starts. Edit it to add an internal CA or a
DNS solver:

```json
{
  "http-01-port": 80,
  "renew-interval-secs": 1800,
  "directories": [
    {
      "id": "letsencrypt-staging",
      "label": "Let's Encrypt staging",
      "url": "https://acme-staging-v02.api.letsencrypt.org/directory",
      "staging": true,
      "allow-private": false
    }
  ],
  "ca-bundle": null,
  "solvers": [
    {
      "id": "manual",
      "kind": "manual"
    }
  ]
}
```

- `allow-private` permits an http(s) URL that resolves to a private address and
  is the internal step-ca opt-in. `ca-bundle` adds PEM roots for that CA; put the
  file under `{data}` (for example `{data}/internal-ca.pem`) or a declared root,
  because the service sandbox does not see the rest of the host.
- `http-01-port` is the port the self-check (and most ingresses) use; it is 80
  unless a QA deployment says otherwise.
- `staging` marks the directory; `issue` refuses a request whose `staging` flag
  disagrees (decision 0007).
- Solvers: `manual` (no configuration), `webhook` (`url`, optional `token`;
  HTTPS only) and `rfc2136` (`server` as host:port, `zone`, `key-name`,
  `secret` base64, optional `algorithm` = `hmac-sha256` (default),
  `hmac-sha512` or `hmac-sha1`). The secret stays in this file at 0600.
- A webhook receives
  `{"action":"present|cleanup","name":"_acme-challenge.example.com.","value":"…","ttl":60}`
  and must answer 2xx.

## Flows

1. `create-account` fetches the directory, signs `newAccount` with the local key
   (ES256 or RS256), stores the key and the CA's account URL, and never returns
   either.
2. `issue` validates the names, refuses a wildcard with HTTP-01 and refuses an
   identical open/valid order unless the request sets `force` (the form exposes
   it as **allow a duplicate order**). It returns a pending order immediately. A worker
   owns the CA conversation: new order, authorizations, challenge, validation,
   finalize, download.
3. HTTP-01 writes the key authorization at 0600, self-checks
   `http://<name>/.well-known/acme-challenge/<token>` and only then tells the CA.
4. DNS-01 computes `_acme-challenge.<name>` and its SHA-256 TXT value. A manual
   solver publishes the instruction in the timeline and waits for
   `confirm-challenge` (up to an hour). RFC 2136 sends a TSIG-signed UPDATE over
   TCP; the webhook is an HTTPS POST.
5. The certificate key is generated locally; the CA only sees a CSR. The chain
   is handed to `certs.store-issued` with `auto-renew`. On success the ACME copy
   of the key is unlinked; if the store is absent or refuses it the order is
   `issued-unstored` and the material stays under `{data}/orders/<id>/` with a
   warning (decision 0006).
6. Renewal runs every `renew-interval-secs`: it reads the store inventory,
   takes only `source = acme` + `auto-renew`, and renews at two thirds of the
   lifetime or in the directory's ARI window, with exponential backoff capped at
   six hours. A staging certificate is never promoted by the loop; a production
   order for it is explicit.
7. `revoke` needs the order that holds the account key and the public chain; the
   reason is an RFC 5280 code (0–10) or its name.

An order in flight when the service restarts is marked invalid rather than
resumed: nonces, authorization cursors and the CA's view cannot be reconstructed
safely.

## Surface

| Route | Function |
| --- | --- |
| `GET /accounts`, `POST /accounts` | list and create an account |
| `GET /orders`, `POST /orders` | list orders and issue |
| `GET /capabilities` | challenges, solvers and directory allowlist |
| `GET /order/events?order=…` | the SSE timeline (record stream) |
| `POST /order/confirm` | release a manual DNS-01 order |
| `POST /order/renew` | start a renewal |
| `POST /order/revoke` | revoke with a reason |

CLI: `anth app acme accounts|account|issue|orders|renew|revoke|confirm`.

The view lists accounts (directory, staging/production, contact, key) and orders
(status, names, challenge, expiry, certificate). Selecting an order follows its
timeline over SSE and falls back to polling; a manual DNS-01 order shows the
exact TXT record with **Confirm DNS record**. **Renew** and **Revoke** live on
the detail, and revoke asks for the reason in a dialog. The FAB's **Issue
certificate** form chooses the directory, names, challenge, solver, key type,
auto-renew and whether a duplicate order is allowed; it creates the account for that directory when missing and hands the
new order to the view with `openView` params. Without a modal slot the view says
so and leaves issuance to the CLI/API.

## Tests

- Unit: account/JWK custody, JWS (ES256 and RS256), EAB, CSR, directory
  allowlist and private refusal, name and wildcard validation, the order record
  shape, event cap, two-thirds and backoff, TSIG framing and token path safety.
- Loopback integration (`cargo test --test loopback`): the real service process
  against a local RFC 8555 fixture that verifies the JWS, fetches the HTTP-01
  token, signs the CSR and revokes; a fake `certs.store-issued` broker proves
  the key arrives and that no key reaches the order file, the routes or the logs.
  It also covers manual DNS-01, duplicate refusal and staging separation.
- Pebble remains the manual QA gate: `scripts/acme-pebble.sh` downloads Pebble
  into `/tmp/anthracite-panel` and prints the environment for an end-to-end run.
  Pebble's CA is trusted through `ca-bundle`; Let's Encrypt staging is a QA row,
  not CI.
- Frontend: `bun test` for the pure helpers; the panel e2e spec for this app
  skips unless the composition carries it.

## Decisions

- [0004](../../docs/decisions/0004-how-does-the-acme-client-allowlist-ca-directories-and-opt-into-a.md)
  — directory allowlist, private opt-in and `ca-bundle`.
- [0005](../../docs/decisions/0005-how-does-a-manual-dns-01-order-get-its-confirmation.md)
  — `confirm-challenge` for the manual solver.
- [0006](../../docs/decisions/0006-where-does-an-issued-certificate-key-live-when-the-certificate-s.md)
  — issued key custody and `issued-unstored`.
- [0007](../../docs/decisions/0007-how-does-the-panel-know-which-ca-directories-are-staging.md)
  — `staging-directories` in the capability.
- [0008](../../docs/decisions/0008-how-is-the-acme-gate-automated-when-ci-cannot-download-or-run-pe.md)
  — the in-repo fixture is the CI gate; Pebble is the real-CA QA gate.

## Limits

- TLS-ALPN-01, IP identifiers and wildcard issuance without DNS-01 are out of
  scope. Vendor DNS adapters are modules of the webhook/RFC 2136 solvers, not
  separate bundles.
- No automatic retry of a failed order: the operator re-issues. Renewal backoff
  is in memory and resets on restart.
- The ACME service does not remove a revoked certificate from the store; the
  certificate manager keeps the inventory and the operator decides.
