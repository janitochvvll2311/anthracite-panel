# 4. Vocabulary

> Part of the [design index](../../DESIGN.md).

| Term | Meaning |
| --- | --- |
| **Engine** | The headless daemon. Mechanism only: config, transport, supervision, runtime, broker, store, audit. No UI. |
| **Panel** | An application that links the engine, serves a web UI and mounts app contributions. Official or custom. |
| **SDK** | The supported surface for panels and apps: Rust guest, Rust panel, Rust CLI, TypeScript bridge and client. |
| **Contract** | The shared, pure vocabulary: identifiers, manifest, composition, grant, world and error. No I/O. |
| **App** | A unit of policy and product: a bundle with a manifest, a component and an authority surface. |
| **Service** | An app kind that runs as a supervised child process for kernel-facing work. |
| **Bundle** | The signed zip: manifest, component, WIT, optional service binary, UI assets. |
| **Manifest** | The app's *proposal*: an index of interfaces, imports, routes, slots and limits. Carries no types. |
| **Interface** | A WIT interface: `namespace:package/interface@semver`. The unit of capability, versioning and grants. |
| **World** | The imports and exports a component actually declares; checked against its grant before linking. |
| **Export** | An interface the app implements. |
| **Import** | An interface the app requires. Must match a provider exactly, by name and type. |
| **Provider** | The app, or `@engine`, that satisfies an import in a composition. |
| **Grant** | The frozen, operator-approved authority derived from a manifest. The runtime contract. |
| **Risk** | The authority level of an interface, computed by the engine over the resolved graph. |
| **Approval** | The operator's explicit acceptance of a risk level; a grant records which ones were presented. |
| **Route** | An HTTP binding from `METHOD /path` to `interface#function`. |
| **Slot** | A named place declared by a panel that an app may render into. |
| **Composition** | The declarative answer to "what is installed, what is composed, who provides what". |
| **Surface** | The runtime result of a composition: routes, slot contributions, CLI verbs, links. |
| **Degraded** | The state of an app whose provider is unavailable at runtime; its routes answer `unavailable`. |
| **Tier** | `engine`, `system`, `third-party`. Determines what authority may be requested at all. |

---
