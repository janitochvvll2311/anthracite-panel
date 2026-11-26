# What exactly does anth-bundle own?

- Date: 2026-11-26T01:10:42.975Z
- Feature: app-sdk (`app-sdk`)
- Status: accepted

## Question

What exactly does anth-bundle own?

## Decision

anth-bundle owns the canonical signed message, the WIT/file hashes, the confined zip unpack and the Ed25519 verification. Among Anthracite crates it depends only on anth-error; its external dependencies are base64, ed25519-dalek, sha2 and zip. Manifest parsing and WIT semantic validation stay in the engine contract/compose, which depend on anth-bundle; tools that need them call anth-conformance.

## Rationale

Publishing anth-bundle must not drag the private contract or wit-parser into a public API, but the signature and the pack/unpack rules are the compatibility boundary every tool shares. Keeping semantic validation engine-side preserves the authority model and the small public surface.

## Alternatives considered

- Publish the contract and wit-parser so anth-bundle can parse manifests; rejected because it exposes the engine authority and a large unstable API.
- Duplicate the signed-message logic in the tools; rejected because a drift would make the engine refuse a valid bundle.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
