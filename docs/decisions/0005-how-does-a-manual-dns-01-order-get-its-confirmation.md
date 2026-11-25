# How does a manual DNS-01 order get its confirmation?

- Date: 2026-11-25T01:52:45.976Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How does a manual DNS-01 order get its confirmation?

## Decision

`issue` returns a pending order, the worker publishes the TXT instruction in a `challenge-ready` event and waits up to an hour. `confirm-challenge(order)` (route `POST /order/confirm`, CLI `confirm`) releases the worker. The webhook and RFC 2136 solvers confirm themselves.

## Rationale

The design's outline has no way to tell the client that the operator copied the record. A dedicated function keeps `issue` asynchronous, makes the wait explicit in the timeline and lets the CLI script it. Orders interrupted by a restart are marked invalid, not resumed.

## Alternatives considered

- Poll the public DNS from the service (needs a resolver and hides propagation errors).
- Block `issue` until the operator acts (the panel request would time out).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
