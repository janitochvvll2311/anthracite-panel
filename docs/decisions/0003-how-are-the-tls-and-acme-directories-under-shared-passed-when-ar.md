# How are the TLS and ACME directories under {shared} passed when argv allows one {shared} placeholder?

- Date: 2026-11-24T19:57:23.102Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How are the TLS and ACME directories under {shared} passed when argv allows one {shared} placeholder?

## Decision

Pass `--shared {shared}` once and derive `{shared}/tls` and `{shared}/acme/http-01` inside the process.

## Rationale

The manifest validator allows at most one {shared} occurrence; certs already derives its publish directory from a single --shared. The grant still shows the shared domain, and the two subpaths are fixed constants.

## Alternatives considered

- Relax the validator to allow two {shared} occurrences.
- Pass them as separate literal paths the operator has to know.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
