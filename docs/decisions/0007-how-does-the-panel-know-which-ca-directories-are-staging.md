# How does the panel know which CA directories are staging?

- Date: 2026-11-25T01:52:46.154Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How does the panel know which CA directories are staging?

## Decision

The capability record carries `staging-directories` in addition to `directories`. The issue form derives the `staging` flag from that list and the backend rejects a request whose flag disagrees with the directory entry.

## Rationale

Guessing from the URL text breaks for an internal staging directory. The operator already configures `staging` per entry; exposing it keeps the UI honest and the request check still guards a forged flag.

## Alternatives considered

- Guess `staging` from the hostname (wrong for internal CAs).
- One separate function per listing (an extra route for one boolean).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
