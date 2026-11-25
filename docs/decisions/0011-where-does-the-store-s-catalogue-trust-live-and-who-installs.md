# Where does the store's catalogue trust live and who installs?

- Date: 2026-11-25T10:30:39.164Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

Where does the store's catalogue trust live and who installs?

## Decision

Each source's Ed25519 public key in `{data}/sources.json` is the catalogue trust root; the index signature is verified over the exact bytes served. The store is read-only: it searches, caches and streams a verified bundle. The panel Manager reads it with the operator session, POSTs the bundle to the engine and shows the engine-computed risk for explicit approval; the store never sees the session or the grant.

## Rationale

An index that could introduce its own key would be self-signed. Keeping installation in the manager preserves D-Manager-Panel's rule that an app module never holds engine management authority, and the engine remains the verifier of the bundle, manifest and WIT.

## Alternatives considered

- Let the store install with a service credential (an app with management authority).
- Trust the index's signer field (the catalogue would vouch for itself).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
