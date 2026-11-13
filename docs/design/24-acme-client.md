# 24. ACME client

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**.
> Start conditions, prerequisites and sequencing: [§21](21-next-version-apps.md).

Bundle `org.anthracite.acme`, package `anthracite:acme@1.0.0`, service class
`socket`, one `panel.view`.

## 1. Scope

- RFC 8555 accounts and orders against any ACME directory: Let's Encrypt,
  Buypass, ZeroSSL (with EAB), Google Trust Services, or an internal step-ca.
- Challenges: HTTP-01 and DNS-01. TLS-ALPN-01 is a later phase.
- Key types: ECDSA P-256 (default), RSA 2048.
- A worker issues, polls, finalizes and downloads; the UI shows a timeline.
- Renewal: read `auto-renew` certificates from the certificate store, renew at
  about two thirds of lifetime or when the ACME Renewal Information (ARI) says
  so, with backoff.
- Revocation with a reason code.
- Staging and production directories; never auto-promote a staging order.

**Non-goals:** being a CA, managing DNS zones, buying domains, TLS-ALPN-01 in
v1, wildcard issuance without DNS-01, or a general-purpose HTTP client for apps.

## 2. Authority and custody

- Class `socket`; outbound TLS to the CA on 443. The directory URLs come from a
  configured allowlist; `create-account` may name one of them, not an arbitrary
  URL. The service refuses loopback, link-local and cloud metadata addresses
  unless the operator opts in (an internal step-ca is the opt-in case).
- Account keys and EAB secrets live in `{data}/accounts/<id>/` at `0600`. They
  are never returned by a route and never logged.
- HTTP-01 does **not** bind port 80. The token directory is
  `--http01-root {shared}/acme/http-01` in argv; the client writes the key
  authorization to `{shared}/acme/http-01/<token>` (`0600`) and relies on the
  ingress that owns port 80 to serve `/.well-known/acme-challenge/<token>`. The
  proxy does this for a host with `acme = true`. Any other listener can read the
  same directory; this is the provider-agnostic seam.
- Before telling the CA the challenge is ready, the client fetches
  `http://<name>/.well-known/acme-challenge/<token>` and compares the body. A
  mismatch is a clear, early failure.
- DNS-01 uses one of three solver kinds, all configured in the service:
  `manual` (the operator copies the TXT record and confirms), `rfc2136` (generic
  dynamic DNS) and `webhook` (an operator-supplied HTTPS endpoint that presents
  and cleans up the record). Vendor-specific adapters are additive modules, not
  separate apps: a separate provider bundle would need its own contract and
  trust decision, and the namespace is only a classification today.
- Issued material goes to the certificate store through
  `anthracite:certs/store#store-issued` over the broker. The key is generated
  locally; the CA never sees it. If the store is absent, the order is
  `issued-unstored` and the material stays in `{data}` with a warning, not
  silently dropped.

## 3. WIT outline

```wit
package anthracite:acme@1.0.0;

interface client {
  use anth:error/kinds@1.0.0.{error};

  enum challenge { http-01, dns-01 }
  enum order-status { pending, ready, processing, valid, invalid, revoked }

  record account {
    id: string,
    directory: string,
    contact: list<string>,
    status: string,
    created-at: u64,
    key-type: string,
  }

  record eab-credentials { kid: string, hmac-key: string }

  record issue-request {
    account: string,
    names: list<string>,
    challenge: challenge,
    solver: option<string>,
    key-type: string,
    auto-renew: bool,
    staging: bool,
  }

  record order {
    id: string,
    account: string,
    names: list<string>,
    challenge: challenge,
    status: order-status,
    expires-at: u64,
    certificate: option<string>,
    last-error: option<string>,
  }

  enum event-kind { created, challenge-ready, validated, issued, failed, renewed, revoked }
  record order-event { order: string, kind: event-kind, at: u64, detail: option<string> }

  record capability { challenge: challenge, solvers: list<string>, directories: list<string> }

  list-accounts: func() -> list<account>;
  create-account: func(directory: string, contact: list<string>, key-type: string, eab: option<eab-credentials>) -> result<account, error>;
  issue: func(request: issue-request) -> result<order, error>;
  renew: func(certificate: string) -> result<order, error>;
  revoke: func(certificate: string, reason: string) -> result<bool, error>;
  list-orders: func() -> list<order>;
  follow: func(order: string) -> result<stream<order-event>, error>;
  capabilities: func() -> list<capability>;
}
```

`create-account` takes `eab.hmac-key` as a write-only secret and never returns it.
`issue` returns immediately; the worker owns the CA conversation. `follow` is
the SSE stream; a poll fallback is allowed for the CLI.

## 4. Routes, CLI and UI

| Surface | Shape |
| --- | --- |
| `GET /accounts`, `POST /accounts` | list and create |
| `GET /orders`, `POST /orders` | list and issue |
| `GET /capabilities` | `capabilities` |
| `GET /order/events?order=…` | `follow`, `stream = true`, `events = true` |
| `POST /order/renew` | renew (body carries the certificate id) |
| `POST /order/revoke` | revoke (body carries the certificate id and reason) |
| CLI `acme accounts`, `issue`, `orders`, `renew` | the same calls |

The view has two panes: accounts and orders. The FAB "Issue certificate" opens a
modal with names, CA directory, challenge, key type, staging and auto-renew. The
order timeline is a stream with a visible poll fallback. The details pane shows
the challenge type and the exact next step for manual DNS, never the account key.

## 5. Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| A1 | Account create, directory fetch, order state, staging | [§22 §1](22-platform-prerequisites.md#1-service-call-broker), [§22 §2](22-platform-prerequisites.md#2-listener-and-egress-intent-in-the-manifest), [§22 §3](22-platform-prerequisites.md#3-secret-custody) |
| A2 | HTTP-01 via the shared token directory, issue, import into certs | A1, [§23 C2](23-certificate-manager.md#5-dependencies-and-phases), proxy data plane |
| A3 | Renewal worker, ARI, backoff, expiry read from certs | A2, [§23 C3](23-certificate-manager.md#5-dependencies-and-phases) |
| A4 | DNS-01 manual + RFC 2136 + webhook, wildcard | A2 |
| A5 | Revocation, audit, hardening | A4, [§22 §5](22-platform-prerequisites.md#5-system-tier-and-service-sandbox), [§22 §7](22-platform-prerequisites.md#7-a-service-audit-bridge) |

## 6. Risks

- **Rate limits.** Let's Encrypt counts failed validations and duplicate
  certificates. The client uses staging by default in the UI, backs off on
  `Retry-After`, and refuses a duplicate order unless the operator asks.
- **Challenge routing.** If the ingress is down or the host does not resolve to
  it, HTTP-01 fails. The self-check turns a CA timeout into an actionable error.
- **Wildcards.** DNS-01 is the only path; the UI must not offer a wildcard with
  HTTP-01.
- **Clock and CA policy.** A skew or an unannounced CA change surfaces as a
  typed failure, not a retry storm.
- **Secret writes.** The account key and EAB secret are write-only. A lost key is
  a new account, not a recovery flow.

## 7. Acceptance

- A staging order against a local ACME test server completes via HTTP-01, and the
  issued chain is imported into the certificate store and published.
- The account key is not present in any route response, log or audit record.
- A failed challenge reports the CA error and the self-check result.
- Renewal runs on schedule, survives a restart, and never renews a certificate
  whose `auto-renew` is false.
- HTTP-01 works with the proxy and with a non-Anthracite listener that reads the
  same token directory.

---
