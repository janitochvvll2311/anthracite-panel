# 5. Architecture at a glance

> Part of the [design index](../../DESIGN.md).

```
┌────────────────────────────── panel (application) ──────────────────────────┐
│  web UI, built with Bun and embedded                                        │
│  slots · iframe host · SDK                                                  │
└────────────────────────────────┬────────────────────────────────────────────┘
                                 │ mounts the engine router in-process
┌────────────────────────────────┴────────────────────────────────────────────┐
│  engine (mechanism, no UI)                                                  │
│  router · sessions · origin · assets                                        │
│  composer · registry · grant store · audit                                  │
│  supervisor · runtime (wasmtime) · broker · store                           │
└────────────────────────────────┬────────────────────────────────────────────┘
                                 │ typed component links + host interface calls
      ┌──────────────────┬───────┴───────┬──────────────────┐
  org.anth.fs      org.anth.terminal   org.anth.hello   third-party app
    (v0.6.0)           (v0.6.0)         (reference)      (wasm + ui)
```

The CLI is another consumer of the same API — in-process today, over HTTP
tomorrow — which is why the engine must never depend on being co-located with its
client.

The SDK sits between both: panels and apps depend on it, and it depends on the
engine's contract, never the reverse.

The lifecycle of one app:

```
bundle ──install──► installed ──approve──► granted ──compose──► running
          (verify)              (freeze)              (wire)
```

Four separate questions, four separate answers — three of them files:

| Question | Answer | Owner |
| --- | --- | --- |
| What content exists? | installed bundles under `apps/` | registry |
| What may it do? | frozen grants under `grants/` | grant store |
| What is active? | `composition.toml` | operator |
| What is reachable now? | the surface | composer |

---
