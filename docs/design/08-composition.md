# 8. Composition

> Part of the [design index](../../DESIGN.md).

### 8.1 The file

```toml
version = 1

[[profile]]
id   = "default"
apps = ["org.anth.fs", "org.anth.terminal", "org.example.notes"]

# Explicit provider choices. Omit to accept the unique provider, if there is one.
[provide]
"anth:fs/list@1.0.0"   = "org.anth.fs"
"anth:fs/read@1.0.0"   = "org.anth.fs"
"anth:env/info@1.0.0"  = "@engine"
```

The panel is not in this file. It is an application, not an app: it reads the
composition and decides where to render each contribution.

`PUT /api/engine/composition` replaces this document, guarded by `ETag` and
`If-Match` (§6.4). Adding or removing an app is a client-side edit of the
document, and the engine validates the result before accepting it.

### 8.2 Resolution

Deterministic, in this order:

1. Load the composition; reject unknown keys (typos are errors).
2. Resolve each app id to exactly one installed manifest; `id@version` pins.
3. Union every import of every composed app, keyed by interface id.
4. For each import, pick a provider: explicit `[provide]` wins; otherwise a
   unique composed exporter; otherwise `@engine` if the interface is in the
   native set; otherwise **unresolved**.
5. Check that the provider's granted export and the consumer's granted import are
   the same interface, exactly.
6. Order apps so providers boot before consumers. Cycles are a composition error:
   typed links cannot be cyclic.
7. Build the surface: routes, slots, CLI verbs, links.
8. Start apps in order with per-app timeouts. A failure marks that app and its
   dependents `degraded` and the boot continues.

### 8.3 Rules

- **One provider per interface.** Two composed providers of the same interface id
  is a configuration error. Silent "first one wins" is how platforms rot.
- **No implicit providers.** `@engine` only for the closed native set.
- **No partial composition.** If an import is unresolved, the consumer does not
  start; it does not start with a stub.
- **Composition does not grant.** Composing an unapproved app is an error that
  names the missing approval.
- **Exact means exact.** No minor-version satisfaction, no structural coercion.

### 8.4 Linking model

- **App → app**: a typed component link, created by the composer only when the
  grant lists that import. No broker, no serialization, no per-call quota.
- **App → host**: an interface call implemented by the broker, which validates
  against the grant, enforces quotas and records denials.

The rule, stated precisely: *the broker is the only exit from the composition;
inside it, authority was exercised when the composition was built.* This is why
the grant must be checked against the component's real world (§7.12): a link is
authority.

Consequence we accept: a trapping provider traps its linked consumers, and
cross-app calls are not individually audited. If that ever needs to change, the
change is to the linker, not to every app.

### 8.5 Missing and failed providers

Two situations that look alike and must not behave alike:

| Situation | Behaviour |
| --- | --- |
| **Static**: at resolve time an import has no provider — disabled, uninstalled, or not composed | **Error.** The composition does not start. `--allow-degraded` is the explicit override. |
| **Dynamic**: a composed provider traps or crashes at runtime | **Degraded.** The engine stays up and consumers answer `unavailable`. |

Disabling a provider is almost always an operator mistake, and a system that
silently boots degraded hides it. A crash is not a mistake to punish: it must
never take the engine down.

A degraded consumer answers with the typed error of §7.11:

```
{"error":"unavailable","message":"anth:fs/list@1.0.0 is not available","app":"org.example.notes"}
```

`anth engine composition` prints which app is degraded and why, without reading
logs.

### 8.6 Recompose

v0.5.0 recomposes by restarting the engine process. `anth compose` says so and
accepts `--now`. It is boring, it is predictable, and it keeps state assumptions
honest. Per-app restart is a later optimization, gated on the store being fully
externalized (it is, by §7.10).

---
