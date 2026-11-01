# 1. The thesis

> Part of the [design index](../../DESIGN.md).

> Anthracite is a headless engine that composes apps. Every product feature is one
> of them, and a panel is the application that hosts them.

The engine owns mechanism: boot, supervision, transport, authority, storage
namespaces, audit. It serves an API, not a UI. Nothing in the engine knows what a
file is, what a terminal is, how a firewall rule looks, or where a navigation item
goes. Those are policies, and policies are apps.

On top of the engine sits a **panel**: an application that links the engine,
serves a web UI and mounts the contributions of the composed apps. The official
panel is ours; the SDK is what makes another one possible.

The product we are building in v0.5.0 is therefore not a feature set. It is the
**scaffold**: the contract, the resolver, the runtime, the broker, the SDK and the
conformance kit that let somebody else write an app, or a whole panel, that we
have never seen and have it run, safely, without editing our code.

**v0.5.0 stops at M3.** It ships the platform, a minimal official panel and the
app that proves them (`hello`). The feature apps that existed in
v0.4.0 return in v0.6.0 as system apps. This is deliberate: the platform is judged
by its contract, not by its catalogue.

Success is measurable:

> **The acid test.** An app is written outside the engine source tree, imports a
> host capability —`anth:env/info` in v0.5.0, the filesystem variant once `fs`
> lands in v0.6.0— contributes one route and one UI slot. It is installed,
> approved and composed. It appears in a panel that was never compiled against it
> — the official one or a custom one — and answers over HTTP and the CLI, with
> **zero changes to any engine source file**.

In v0.5.0 we execute the acid test ourselves, against the official panel and a
minimal custom one. In v0.6.0 somebody who is not us must be able to.

---
