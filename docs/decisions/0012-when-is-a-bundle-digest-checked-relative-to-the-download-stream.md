# When is a bundle digest checked relative to the download stream?

- Date: 2026-11-25T10:30:39.173Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

When is a bundle digest checked relative to the download stream?

## Decision

The service fetches or reads the whole bundle (32 MiB cap), checks the size and SHA-256 against the index, caches it by digest and only then streams the verified bytes. A mismatch is an error before the first chunk; a cached bundle is re-hashed on every read and evicted when corrupt.

## Rationale

The manager must never receive a partial or unverified bundle it could POST to the engine. Verifying first costs one buffer but makes the handoff a single trusted artifact and keeps a tampered cache from being served.

## Alternatives considered

- Hash while streaming and abort at the end (the manager already has bytes).
- Trust the cache filename (a corrupt or replaced file would pass).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
