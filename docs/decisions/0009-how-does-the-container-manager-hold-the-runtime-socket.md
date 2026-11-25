# How does the container manager hold the runtime socket?

- Date: 2026-11-25T05:27:28.057Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How does the container manager hold the runtime socket?

## Decision

`--runtime` is a literal argv entry and the manifest declares the same path as a root without `:ro` (read/write), class `admin`. The approval names the socket and says it is root-equivalent. The sandbox applies only file rights to a non-directory root, so the service can connect and nothing else on the host becomes visible.

## Rationale

The socket is not a network endpoint and cannot be granted by egress; it has to be a declared filesystem root. A read-only rule would not be enough for the connect permission, and hiding the path would make the approval lie. The file-rights mask is what Landlock accepts on a socket.

## Alternatives considered

- Proxy the Engine API through the engine (a second API and a second trust boundary).
- Shell out to the docker CLI (no typed errors, ambient environment).

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
