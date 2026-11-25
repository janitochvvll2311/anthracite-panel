# How do exec sessions work and how is the runtime tested without Docker?

- Date: 2026-11-25T05:27:28.071Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How do exec sessions work and how is the runtime tested without Docker?

## Decision

The service creates the exec, starts it over a hijacked Engine stream and owns the session (read task, write half, buffer, resize and close); the engine only relays bytes. CI runs a fixture Engine API over a Unix socket for lifecycle, logs, stats and exec; Docker and rootless Podman remain manual QA rows.

## Rationale

Exec is bidirectional on one connection, which the route model cannot express as a call; a service-owned session keeps resize and close typed. Docker cannot be installed on every test host, so a fixture is the always-on gate and the real runtimes stay deliberate QA.

## Alternatives considered

- One request per exec keystroke (no terminal semantics).
- Require Docker in CI (not hermetic; the container-in-container gap).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
