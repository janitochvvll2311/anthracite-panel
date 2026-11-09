# Anthracite v0.5.0 — design index

Status: **accepted**. This is the entry point to the design of record. Each
section lives in its own document under `docs/design/`; this file summarises the
design and maps every section to its file.

Decisions are binding. They are recorded in [`16-decision-log.md`](docs/design/16-decision-log.md),
and a change to one of them is a design change, not an implementation detail.

---

## The design in one page

- **The engine is headless.** It composes apps and serves an API. It has no UI, no
  slots, no idea of what a file or a terminal is.
- **A panel is an application** that links the engine, serves a web UI and mounts
  the contributions of the composed apps. Ours is official; the SDK is what makes
  another one possible.
- **An app is a signed component bundle**: a manifest that indexes its interfaces,
  WIT that defines them, a WebAssembly component that implements them, optional UI
  modules and an optional service binary.
- **WIT is the only type system.** Interfaces are versioned and matched exactly,
  and the component's real world is checked against its grant before it runs.
- **Composition is data.** `composition.toml` says what runs and who provides
  what; the engine resolves it deterministically and fails loudly on a static
  miss, degrading only on runtime failures.
- **Authority is frozen in a grant**, computed from the resolved import graph
  rather than declared by the app. Installing, approving and composing are three
  separate, auditable acts.
- **The broker is the only exit from the composition**; inside it, app-to-app
  calls are typed component links created at compose time.
- **The SDK is the compatibility boundary.** Panels and apps depend on it, never
  on engine internals, and it is transport-neutral so a remote panel is a
  transport question rather than a redesign.
- **v0.5.0 ships M0–M3**: the platform, a minimal official panel and the apps that
  prove both. Feature apps return in v0.6.0.
- **The acid test**: an app written outside the engine tree appears and answers
  inside a panel that was never compiled against it, with zero changes to engine
  code.

---

## Documents

| § | Document | What it settles |
| --- | --- | --- |
| 1 | [Thesis](docs/design/01-thesis.md) | Why this is a platform and not a panel, and what success measures |
| 2 | [Why restart](docs/design/02-why-restart.md) | What v0.4.0 proved, and what we deliberately drop |
| 3 | [Principles](docs/design/03-principles.md) | The twelve rules that hold under pressure |
| 4 | [Vocabulary](docs/design/04-vocabulary.md) | Engine, panel, SDK, app, interface, grant, slot, tier |
| 5 | [Architecture](docs/design/05-architecture.md) | How the pieces fit and the lifecycle of one app |
| 6 | [Engine](docs/design/06-engine.md) | Subsystems, guardrails, runtime backend, HTTP surface, boot, contract crate |
| 7 | [App](docs/design/07-app.md) | Bundle, manifest, WIT, kinds, routes, slots, events, CLI, state, errors, enforcement, mapping, UI |
| 8 | [Composition](docs/design/08-composition.md) | The file, resolution, rules, linking, failures, recompose |
| 9 | [Authority](docs/design/09-authority.md) | Tiers and signing, native set, grants, prohibitions, updates |
| 10 | [Workspace](docs/design/10-workspace.md) | The four Rust and three Bun workspaces, forbidden edges, one target dir |
| 11 | [Roadmap](docs/design/11-roadmap.md) | M0–M5, the definition of done, and what gates what |
| 12 | [Testing](docs/design/12-testing.md) | Contract tests, golden compositions, the conformance kit |
| 13 | [Migration](docs/design/13-migration.md) | What survives from v0.4.0 and the safety invariants it must keep |
| 14 | [Risks](docs/design/14-risks.md) | What could go wrong and the answer to each |
| 15 | [Open decisions](docs/design/15-open-decisions.md) | What is still undecided, with a recommendation and a deadline |
| 16 | [Decision log](docs/design/16-decision-log.md) | Every binding decision taken during the review |
| 17 | [Deferred](docs/design/17-deferred.md) | Everything postponed to v0.6.0, with the reason and the trigger |
| 18 | [v0.6.0 plan](docs/design/18-v0.6-plan.md) | M4–M5 phases, entry conditions and the open decisions |
| A | [Vertical slice](docs/design/appendix-a-vertical-slice.md) | `hello` end to end: install, approve, compose, call |

---

## Reading paths

| If you are… | Read |
| --- | --- |
| New to the project | 1 → 5 → 6 → 8 |
| Writing an app | 7 → A |
| Writing a panel | 5 → 6 → 7 (7.7, 7.14) → 10 |
| Operating a deployment | 6 (6.4, 6.5) → 8 → 9 → 15 |
| Reviewing a change | 16, then the document for the area |

---

## Maintaining these documents

- **Section numbers are stable identifiers.** `§7.12` is referenced from code,
  tests and problems; numbers are never reused, and a removed section leaves a
  gap rather than renumbering the rest.
- **Each document starts with its original heading**, so a reference like
  `§6.4` stays searchable across the split.
- **A decision lands in two places**: its row in the decision log, and the
  document it changes. The log is the history; the document is the rule.
- **A document over roughly 250 lines splits.** Today only §6 (237) and §7 (406)
  are close or over, and they are the first candidates.
- **The table above is the map.** Renaming a document means updating its row.
