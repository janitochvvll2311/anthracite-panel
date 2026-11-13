# 23. Certificate manager

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**.
> Start conditions, prerequisites and sequencing: [§21](21-next-version-apps.md).

Bundle `org.anthracite.certs`, package `anthracite:certs@1.0.0`, service class
`admin`, one `panel.view`.

## 1. Scope

- Inventory certificates from approved roots: `/etc/ssl/certs`,
  `/etc/letsencrypt/live`, a service import directory, and the managed store.
- Show names, issuer, validity, key type, fingerprint and chain state.
- Import a PEM chain and key, validate that they match, and publish them for
  consumers.
- Remove a managed certificate, with a confirmation.
- Mark a managed certificate `auto-renew`; the ACME app reads that flag. Only
  `source = acme` certificates can be marked, so a manual import is never
  renewed behind the operator's back.
- Watch expiry and surface `valid`, `expiring` (30/14/7 days), `expired`,
  `invalid`.

**Non-goals:** being a CA, issuing internal certificates, modifying the system
trust store, OCSP/CRL infrastructure, key escrow, HSM support, or showing a
private key anywhere in the panel.

## 2. Authority and custody

- Class `admin`; the roots are literal `--scan` argv entries and the publish
  directory is `--publish {shared}/tls`, all frozen in the grant and visible at
  approval.
- Writes go to `{data}/certs/<id>/` (`0700`) and are published atomically to
  `{shared}/tls/<id>/`:

  ```
  {shared}/tls/<id>/fullchain.pem   0600
  {shared}/tls/<id>/privkey.pem     0600
  {shared}/tls/<id>/meta.json       0600
  ```

- `{shared}` is the service trust domain. An app that is granted `{shared}` can
  read the published material, so the grant is shown as a service-to-service
  boundary, not as a convenience.
- The private key is never returned by the service through WIT or HTTP. It
  enters through an explicit import or an ACME issuance; the only consumer path
  is the published directory, read by the proxy service.
- A system certificate is **inventory only** unless the operator imports a copy.
  The manager does not copy keys out of `/etc/letsencrypt` behind the operator's
  back.

## 3. WIT outline

```wit
package anthracite:certs@1.0.0;

interface store {
  use anth:error/kinds@1.0.0.{error};

  enum source { acme, imported, system }
  enum status { valid, expiring, expired, invalid }

  record certificate {
    id: string,
    names: list<string>,
    issuer: string,
    not-before: u64,
    not-after: u64,
    key-type: string,
    source: source,
    fingerprint-sha256: string,
    status: status,
    auto-renew: bool,
  }

  record import-request {
    cert-pem: string,
    chain-pem: string,
    key-pem: string,
    names: list<string>,
    auto-renew: bool,
  }

  record validation {
    ok: bool,
    reason: option<string>,
    chain-ok: bool,
    key-matches: bool,
  }

  inventory: func() -> list<certificate>;
  get-certificate: func(id: string) -> option<certificate>;
  import-certificate: func(request: import-request) -> result<certificate, error>;
  store-issued: func(request: import-request) -> result<certificate, error>;
  remove-certificate: func(id: string) -> result<bool, error>;
  validate: func(id: string) -> validation;
  publish: func(id: string) -> result<certificate, error>;
  set-auto-renew: func(id: string, enabled: bool) -> result<certificate, error>;
}
```

`error` is the contract error from `anth:error`, mapped to the envelope by the
service SDK. `import-certificate` is the operator import and sets
`source = imported`; `store-issued` is the ACME entry point and sets
`source = acme`. The service owns the source, not the caller. Both are audited
through the service audit bridge ([§22 §7](22-platform-prerequisites.md#7-a-service-audit-bridge)), and the request body is never logged.

## 4. Routes, CLI and UI

| Surface | Shape |
| --- | --- |
| `GET /certs` | `inventory` |
| `GET /certificate?id=…` | `get-certificate` |
| `POST /certs` | `import-certificate` |
| `DELETE /certificate?id=…` | `remove-certificate` |
| `POST /certificate/validate` | `validate` (body carries the id) |
| `POST /certificate/publish` | `publish` (body carries the id) |
| `POST /certificate/auto-renew` | `set-auto-renew` (body carries the id and flag) |
| CLI `certificates list`, `show`, `import`, `validate` | the same calls |

The CLI column is illustrative: every verb is a `[[cli]]` entry naming the same
`interface#function` as the route above it, so a verb without a route is
`unavailable` (D-Cli-Invocation).

The view is a table with status badges, expiry, source and a details modal. The FAB is icon-only: one action opens the import modal, whose file picker
uses `files = true`; the module reads the PEM text and calls its own service. A
second action refreshes the inventory. The modal warns that the key transits the
browser and recommends ACME or the service import directory for production.

## 5. Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| C1 | Inventory, parsing, validation, table, details | [§22 §2](22-platform-prerequisites.md#2-listener-and-egress-intent-in-the-manifest), [§22 §3](22-platform-prerequisites.md#3-secret-custody) |
| C2 | Import, managed store, publish to `{shared}/tls` | C1, [§22 §3](22-platform-prerequisites.md#3-secret-custody) |
| C3 | Auto-renew flag, expiry warnings, remove guards; consumer counts wait for a proxy-published `consumers.json` in `{shared}` (not a typed import) | C2 |
| C4 | System-root scan, audit bridge, release hardening | C3, [§22 §5](22-platform-prerequisites.md#5-system-tier-and-service-sandbox), [§22 §7](22-platform-prerequisites.md#7-a-service-audit-bridge) |

## 6. Risks

- **Key in the browser.** Manual import means the key is in the page's memory.
  The UI says so. The preferred paths are ACME or dropping the pair into the
  service import directory.
- **Shared directory trust.** `{shared}` is not per-consumer. The grant text must
  say that publishing a key makes it readable by every service with `{shared}`.
- **System roots vary.** Distros disagree on `/etc/ssl/certs` and
  `/etc/letsencrypt`, and the engine user may not be able to read them. The
  defaults are scan roots, not promises; the operator edits argv or fixes the
  permission boundary. The service reports an unreadable root instead of
  pretending it is empty.
- **Remove while in use.** A removed certificate leaves the published files until
  the proxy reloads; the panel warns, and the proxy keeps its last-good config
  until the reload.

## 7. Acceptance

- A PEM pair imports, validates and appears in `{shared}/tls/<id>` with `0600`.
- A mismatched key/cert is refused with a typed error and no files written.
- The panel never displays the private key and no route returns it.
- Removing a certificate used by a proxy host: the panel asks for confirmation;
  the proxy keeps the last-good configuration until it reloads.
- The service survives a malformed PEM, a missing root and a read-only `{shared}`
  without taking down the engine.

---
