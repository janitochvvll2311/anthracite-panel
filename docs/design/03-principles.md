# 3. Principles

> Part of the [design index](../../DESIGN.md).

1. **Mechanism in the engine, policy in apps.** If it can be expressed as an app,
   it must be.
2. **One contract, no side doors.** There is no privileged path around the app
   contract — not for our own apps, not for the panel.
3. **The manifest is a proposal; the grant is the contract.** Runtime behaviour is
   validated against the frozen grant, and the component's real type must match
   it before anything is linked.
4. **Composition is data.** What runs, and who provides what, is a file an
   operator can read, diff and review — not code.
5. **The type system is WIT.** We do not invent a second one. Interfaces are
   versioned, exact and checked structurally.
6. **Interfaces are small and single-purpose.** The grant scopes at interface
   granularity, so a god-interface would be a god-grant.
7. **The native capability set is closed.** The engine provides a small, fixed
   list of environment interfaces (§9.2). Growing that list is a design change.
8. **Failure is local at runtime.** A degraded app never takes the engine down; a
   broken composition is a configuration error and fails fast (§8.5).
9. **The platform is done when adding an app or a panel needs no engine change.**
10. **Everything is observable.** Composition state, grants and denials are
    queryable and auditable without reading logs.
11. **The engine serves no UI.** Not a page, not a section, not a slot. A panel
    does that, and a panel is replaceable.
12. **The SDK is the compatibility boundary.** A panel or an app depends on the
    SDK, never on engine internals — and the SDK must work over HTTP, so a remote
    panel is a transport question, not a redesign.
13. **No ambient authority.** An app never receives a path, an implicit identity
    or an inherited environment. Everything it can reach is an interface it was
    granted.

---
