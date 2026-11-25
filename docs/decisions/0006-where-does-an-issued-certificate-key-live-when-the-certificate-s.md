# Where does an issued certificate key live when the certificate store is absent?

- Date: 2026-11-25T01:52:46.032Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

Where does an issued certificate key live when the certificate store is absent?

## Decision

The key is generated locally (never sent to the CA) and written as PKCS#8 PEM at 0600 under `{data}/orders/<id>/privkey.pem`. `certs.store-issued` receives it through the broker; on success the copy is unlinked and only the public chain remains. On failure the order becomes `issued-unstored` and the key stays with a warning.

## Rationale

Silently dropping a key after the CA counted the issuance would waste a rate limit and leave the operator with nothing. Keeping it only in the failure path bounds the duplicate custody; the public chain is kept for ARI and revocation.

## Alternatives considered

- Fail the order (the certificate exists at the CA and cannot be downloaded again).
- Keep every key in the ACME data directory (duplicate custody by default).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
