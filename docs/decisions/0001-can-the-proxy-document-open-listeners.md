# Can the proxy document open listeners?

- Date: 2026-11-24T19:57:23.082Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

Can the proxy document open listeners?

## Decision

The document selects listeners only among the argv binds frozen in the manifest (tcp:8080/tcp:8443); it can map hosts onto them but an invented bind is a validation error.

## Rationale

The authority to bind a port belongs to the manifest and the approval, not to an app-owned document. It also keeps the CAP_NET_BIND_SERVICE decision at launch and means an apply cannot conflict with another daemon's port or widen the granted surface.

## Alternatives considered

- Let the document bind arbitrary ports and reconcile sockets on apply.
- Bind one wildcard listener and route by SNI/Host only.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
