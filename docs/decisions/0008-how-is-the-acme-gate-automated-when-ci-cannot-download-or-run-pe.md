# How is the ACME gate automated when CI cannot download or run Pebble?

- Date: 2026-11-25T02:40:59.242Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How is the ACME gate automated when CI cannot download or run Pebble?

## Decision

The always-on gate is an in-repo RFC 8555 fixture (`apps/acme/backend/tests/loopback.rs`) that verifies the JWS, serves HTTP-01 and signs the CSR. Pebble is the real-CA gate: `scripts/acme-pebble.sh` downloads and prepares it, and `tests/pebble.rs` (`--ignored`) runs the same client against it. Both run in release QA; a machine without Pebble still exercises the protocol in CI.

## Rationale

Pebble is a Go binary and a download; the repository has no runner and no vendor tree for it. A silent skip would let a broken client pass. The fixture keeps the protocol path tested on every run, while the ignored test and script make the real CA a deliberate QA step rather than a claim.

## Alternatives considered

- Vendor the Pebble binary (architecture-specific bytes in the tree).
- Download Pebble in every test run (network-dependent and slow).
- Skip the real CA (loses the only external conformance signal).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
