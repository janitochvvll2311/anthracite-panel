# How does the document express HTTPS redirect, HSTS, forwarded headers, body limits and lost log events?

- Date: 2026-11-24T19:57:23.094Z
- Feature: v0.7.0 (`v0-7-0`)
- Status: accepted

## Question

How does the document express HTTPS redirect, HSTS, forwarded headers, body limits and lost log events?

## Decision

Extend the design's records: `host.redirect-https`, `host.hsts`, `listener.forwarded`, `route.body-limit` and `proxy-status.dropped`.

## Rationale

The acceptance criteria require an HTTP-to-HTTPS redirect, header policy, per-route body limits and a dropped-event count, while the §3/§4 outline had no field for them. The extensions stay in the same versioned document; there is no second config surface.

## Alternatives considered

- A second policy document.
- Implicit redirect/HSTS for every host with a certificate.
- A global limit instead of per route.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
