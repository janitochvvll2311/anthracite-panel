# How does the ACME client allowlist CA directories and opt into an internal CA?

- Date: 2026-11-25T01:52:45.956Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How does the ACME client allowlist CA directories and opt into an internal CA?

## Decision

The allowlist is `{data}/config.json`: each entry has `url`, `label`, `staging` and `allow-private`. Only a listed URL can be used by `create-account`. A non-HTTPS URL and any loopback/link-local/private/metadata target are refused unless that entry sets `allow-private`, and an internal CA root is trusted only through the optional `ca-bundle` PEM, copied under `{data}` (or a declared read-only root) because the service sandbox does not see the rest of the host.

## Rationale

The WIT takes a directory URL, but an arbitrary URL would let a route aim the client at an internal endpoint. The opt-in keeps a step-ca possible without weakening the default. Resolving a public name to a private address is refused too, so the allowlist is not a DNS trust anchor.

## Alternatives considered

- Hardcode the public CA list (no internal CA).
- Trust the OS store (not hermetic, and a test cannot isolate it).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
