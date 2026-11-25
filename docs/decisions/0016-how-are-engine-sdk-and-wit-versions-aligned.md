# How are engine, SDK and WIT versions aligned?

- Date: 2026-11-25T18:56:46.759Z
- Feature: app-sdk (`app-sdk`)
- Status: accepted

## Question

How are engine, SDK and WIT versions aligned?

## Decision

The v0.8.0 line carries engine, sdk/rust and panel 0.8.0; the SDK crates start at 0.8.0. WIT interfaces keep their own versions. 0.8.x SDK is compatible with 0.8.x engine; a breaking SDK change bumps the minor while the line is 0.x.

## Rationale

One line number makes the compatibility matrix readable, while WIT interfaces evolve on their own contract. Published versions are immutable.

## Alternatives considered

- An independent SDK version starting at 1.0; rejected because the app contract is still 0.x and would imply a stability it does not have.
- Pin every package to the exact engine patch; rejected because it turns every engine patch into an SDK release.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
