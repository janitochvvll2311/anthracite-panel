# What is the public SDK surface for third-party Anthracite apps?

- Date: 2026-11-25T18:56:46.734Z
- Feature: app-sdk (`app-sdk`)
- Status: accepted

## Question

What is the public SDK surface for third-party Anthracite apps?

## Decision

Publish anth-error, anth-guest, anth-service, anth-client, anth-bundle and cargo-anth on crates.io, and @anth-panel/panel, @anth-panel/client, @anth-panel/wit, @anth-panel/cli, @anth-panel/create-app and @anth-panel/bun-plugin on npm. Keep anth-api, anth-engine, anth-runtime, anth-compose, the full anth-contract and the Rust anth-panel private. anth-error is the small exception extracted from anth-contract; anth-bundle lives in engine/bundle so the engine can depend on it without an engine -> sdk edge.

## Rationale

Third parties need the guest/service libraries, the transport and the panel contract, not the engine's authority or implementation. The small surface is what can keep a semver promise.

## Alternatives considered

- Publish every engine crate so conformance and signing are libraries too; rejected because it exposes authority and makes every internal refactor a breaking SDK change.
- Publish nothing and use git dependencies; rejected because monorepo path dependencies and moving tags are fragile for third parties.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
