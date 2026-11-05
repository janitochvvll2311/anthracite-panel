# 8. Composition

> Part of the [design index](../../DESIGN.md).

### 8.1 The file

```toml
version = 1

[[profile]]
id   = "default"
apps = ["org.anth.hello", "org.example.notes"]

# Pin a version when the app must not drift: "org.example.notes@0.1.0".
```

The panel is not in this file. It is an application, not an app: it reads the
composition and decides where to render each contribution.

There is no provider map. **The interface id is the binding**: an import is
satisfied by the unique composed app that exports it, or by `@engine` when the
interface is in the native set. A second implementation is a different interface,
not a second provider of the same one.

`PUT /api/engine/composition` replaces this document, guarded by `ETag` and
`If-Match` ([§6.4](06-engine.md#64-http-surface)). Adding or removing an app is a client-side edit of the
document, and the engine validates the result before accepting it.

v0.5.0 has exactly one profile. The array form exists because per-user profiles
are a known open decision ([§15](15-open-decisions.md#15-open-decisions)), and the validator rejects a second profile until
then.

### 8.2 Resolution

Deterministic, in this order:

1. Load the composition; reject unknown keys (typos are errors) and a second
   profile.
2. Resolve each app id to exactly one installed manifest; `id@version` pins.
3. Union the imports of every app that will run — approved and enabled. An app
   that cannot run contributes no imports and no exports.
4. For each import, look for candidates among the composed apps that export it,
   whatever their state:
   - none and the interface is native → `@engine`;
   - none and the interface is not native → **unresolved**;
   - one that runs → link;
   - one that cannot run (unapproved or disabled) → its consumers are **blocked**;
   - more than one → **duplicate**.
5. Check that the provider's granted export and the consumer's granted import are
   the same interface, exactly.
6. Order apps so providers boot before consumers. Cycles are a composition error:
   typed links cannot be cyclic.
7. Build the surface: routes, slots, CLI verbs, links.
8. Start apps in order with per-app timeouts. A failure to instantiate marks that
   app and its dependents `degraded` and the boot continues.

`unresolved` and `duplicate` are fatal; `blocked` and a failed instantiation are
degraded. The reason is in [§8.5](08-composition.md#85-missing-and-failed).

### 8.3 Rules

- **One provider per interface.** Two composed apps exporting the same interface
  id is a configuration error, named with both apps. Silent "first one wins" is
  how platforms rot.
- **The interface id is the binding.** `@engine` is the provider for the closed
  native set, implicitly. An app cannot export `anth:*`, so there is never a
  choice to make and nothing to declare.
- **An app that cannot run provides nothing usable.** Unapproved or disabled is
  not the same as absent: it turns what would be a fatal miss into a degradation,
  because the fix needs the engine that is booting ([§8.5](08-composition.md#85-missing-and-failed)).
- **Composition does not grant.** Composing an unapproved app is allowed — it will
  be degraded — but the engine never writes a grant to make it run.
- **Exact means exact.** No minor-version satisfaction, no structural coercion.

### 8.4 Linking model

- **App → app**: a typed component link, created by the composer only when the
  grant lists that import. No broker, no serialization, no per-call quota.
- **App → host**: an interface call implemented by the broker, which validates
  against the grant, enforces quotas and records denials.

The rule, stated precisely: *the broker is the only exit from the composition;
inside it, authority was exercised when the composition was built.* This is why
the grant must be checked against the component's real world ([§7.12](07-app.md#712-enforcement-the-world-must-match-the-grant)): a link is
authority.

Consequence we accept: a trapping provider traps its linked consumers, and
cross-app calls are not individually audited. If that ever needs to change, the
change is to the linker, not to every app.

### 8.5 Missing and failed

Failure is classified by phase, and by one question: **can the operator fix it
without the engine running?** If the only fix is an engine operation, failing the
boot would lock the operator out of the tool that repairs the system.

| Phase | Situation | Behaviour |
| --- | --- | --- |
| Resolve | an import has **no candidate** at all | **fatal**, unless `--allow-degraded` |
| Resolve | **two** apps export the same interface | **fatal** |
| Resolve | the composition names an app that is **not installed** | **fatal** |
| Resolve | an import's only candidate is **unapproved** or **disabled** | **blocked**, degraded |
| Instantiate | the component fails to load, link or start | degraded, audited |
| Run | a call traps, times out or exhausts its budget | degraded |

The three fatal cases share a property: **the composition file is the error
message**. They are fixed by editing a TOML or installing an app, without a
daemon. The blocked case is the opposite: approving an app or enabling it is an
engine operation, so the engine has to boot to be fixable.

"A degraded app" means it is not running: there is no instance that could answer,
and its consumers receive `unavailable`. `anth status` names the app and the
reason without reading logs.

Instantiating a component that fails its world check against the grant is a
security event, not a configuration error: it is audited and the app degrades. One
app must not be able to stop the engine from starting.

A degraded consumer answers with the error envelope of [§7.11](07-app.md#711-errors):

```json
{"error":{"code":"unavailable",
          "message":"anthracite:fs/list@1.0.0 is not available",
          "app":"org.example.notes"}}
```

### 8.6 Recompose

v0.5.0 recomposes by restarting the engine process. `anth compose` says so and
accepts `--now`. It is boring, it is predictable, and it keeps state assumptions
honest.

It is **not** justified yet by "the store is externalized": apps may hold state in
a long-lived instance ([§7.10](07-app.md#710-state)). Per-app restart becomes safe when apps treat the
store as their only durable state, and that is a property of the apps, not of the
engine. Until then, a restart loses in-memory app state, and that is the accepted
cost.

### 8.7 The surface view

`GET /api/engine/composition` returns the document and the resolution the boot
used, so a client does not infer state from logs:

```json
{
  "composition": {"version": 1, "profile": [{"id": "default", "apps": ["org.example.notes"]}]},
  "surface": {
    "apps": [{"id": "org.example.notes", "version": "0.1.0", "risk": "read"}],
    "degraded": [{"app": "org.example.consumer", "version": "0.2.0", "reason": "blocked",
                  "detail": {"interface": "example:notes/notes@1.0.0",
                             "provider": "org.example.notes"}}],
    "links": [{"consumer": "org.example.consumer",
               "interface": "example:notes/notes@1.0.0",
               "provider": "org.example.notes"}]
  }
}
```

`apps` is the boot order: providers before consumers. `degraded` carries the
kebab-case reason of §8.5 — `unapproved`, `grant-stale`, `grant-invalid`,
`disabled`, `blocked` or `risk-grown` — plus `detail` when the code alone is not
enough (the blocked link, or the approved and effective risk). `links[*].provider`
is `engine` for the native set and the provider's id otherwise. A fatal
resolution answers with the envelope of [§7.11](07-app.md#711-errors), so the
endpoint and the boot agree on the three cases that cannot be degraded.

---
