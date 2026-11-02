# 14. Risks

> Part of the [design index](../../DESIGN.md).

| Risk | Mitigation |
| --- | --- |
| The contract becomes a framework, and apps fight it. | Ship `hello` and the minimal panel first; every ergonomic complaint is a contract change, not a local workaround. |
| The SDK becomes a public API we must keep stable. | It only exposes what a panel or an app needs; the minimal custom panel in M3 is the compatibility test, not a demo. |
| The engine grows an in-process shortcut that a remote panel cannot use. | The SDK panel API is transport-neutral from M0; the edge test and the M3 custom panel both run against it. |
| The component model moves under us (WASI 0.3 async landed in 2026). | Pin wasmtime; treat async as a v0.6.0 dependency, not a v0.5.0 one. |
| JIT requires executable memory. | Documented requirement; the Pulley profile is the escape hatch and the scaffolding stays. |
| Binary size grows with a wasm compiler. | Only the daemon pays; curated wasmtime features, `opt-level=s`, LTO, strip, measured in M0. |
| Exact interface versioning forces coordinated upgrades. | Providers export two versions during a transition; that is the documented path. |
| Dynamic WIT glue (events) is expensive to build. | Deferred to v0.6.0 with streams as the intended mechanism. |
| The pre-shaped runtime boundary turns out to be wrong. | A `Runtime` trait with a fake backend is exercised by engine tests from M0, and the M0 async probe checks the real toolchain before M5 depends on it. |
| Services reintroduce ambient authority. | Tier rules: third party never gets a service in v0.5.0 or v0.6.0. |
| We rebuild v0.4.0 feature-first and lose the platform. | M0–M3 gate M4: no system app starts before the acid test passes. |
| The headless path rots because only the linked panel is built and tested. | The SDK is transport-neutral from M0, the engine boots with no UI, and `doctor` plus the HTTP suite run against a headless daemon in CI. |
| The acknowledgment becomes reflex noise and stops protecting anything. | It only appears when consumers would actually break, and every use is audited with the names it broke. |

---
