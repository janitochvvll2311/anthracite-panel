# What code is shared between the engine and the SDK tools?

- Date: 2026-11-25T18:56:46.751Z
- Feature: app-sdk (`app-sdk`)
- Status: accepted

## Question

What code is shared between the engine and the SDK tools?

## Decision

Extract manifest parsing, the canonical manifest.sig message, WIT hashing, bundle reading and digest rules into anth-bundle. It lives in engine/bundle and is published there, so the engine can depend on it without violating the engine -> sdk prohibition. The error kind and envelope move to the small public anth-error crate in engine/error, re-exported by anth-contract, so anth-service can be published without exposing the full contract. The engine, anth-sign, anth-conformance and cargo-anth depend on anth-bundle.

## Rationale

A signature produced by the SDK must be the one the engine verifies. Duplicating the signed message would drift silently at the first manifest change.

## Alternatives considered

- Have the CLI shell out to anth-sign and anth-conformance; accepted only as the Phase 1 bridge, then replaced by anth-bundle.
- Duplicate the logic in the SDK; rejected because the signature is a compatibility boundary.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
