# 2. Why restart

> Part of the [design index](../../DESIGN.md).

v0.4.0 proved the hard parts: one HTTP dialect over socket and TCP, typed
exports and imports, frozen grants, a wasm runtime with a capability bridge, and
a signed bundle format. It also demonstrated the failure mode we are escaping:
every feature edited the host crate, the router, the CLI and the shell, because
features lived *inside* the product instead of *on* it.

v0.5.0 keeps the *lessons* and discards the *structure*:

- keep: one HTTP dialect, typed contracts, frozen grants, signature and hash
  verification, hash-pinned assets, the "manifest is a proposal, the grant is the
  contract" rule;
- discard: feature code in the host, a monolithic `AppService`, a router that
  knows feature routes, a shell that knows feature sections, a home-grown type
  language and a hand-rolled ABI.

---
