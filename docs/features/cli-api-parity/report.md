# Report — CLI and API parity

## Summary

This pass compared the engine API, the `anth` CLI, the panel surface and the app
surfaces. App verbs are clean: every `[[cli]]` verb resolves to a route bound to
the same `interface#function`, and the committed system apps have no missing
frontend calls. The remaining gaps are pre-existing engine commands and the
absence of generated clients for the system apps.

## Matrix

| Surface | Verdict |
| --- | --- |
| `anth status` ↔ `/api/health`, `/api/engine/version`, `/api/engine/composition`, `/api/engine/apps` | pass |
| `anth compose` ↔ `GET`/`PUT /api/engine/composition` | pass |
| `anth key list/add/remove` ↔ keys resources | pass |
| `anth app list/show/install/upgrade/enable/disable/remove/approve/revoke` ↔ apps resources | pass |
| `anth app <alias> <verb>` ↔ app route | pass |
| `anth audit` ↔ `GET /api/engine/audit` | pass in #163 |
| `anth doctor` | offline; no API by design |
| `anth restart` ↔ `POST /api/engine/restart` | **gap**: neither route nor verb exists |
| `anth login/logout` ↔ session resource | **gap**: API exists, CLI is Unix-socket only |
| operator password ↔ `GET`/`PATCH /api/engine/operator` | **gap**: API exists, no CLI command |
| Panel shell routes (`/login`, `/setup`, `/panel/config.json`, assets) | panel-only; no CLI equivalent |
| Panel frontend → engine API | pass; audit call is in #163 |
| App `[[cli]]` → `[[route]]` | pass: 38 verbs, 0 missing |
| App frontend → own routes | pass: no missing route strings in the eight system apps |
| Generated clients | hello only; current with codegen. System apps use hand-written `context.fetch` |

## Delivered

- `engine/api/tests/golden.rs` reads every real `apps/<name>/manifest.toml(.in)`
  and fails if a `[[cli]]` call has no matching `[[route]]`. This is the parity
  rule the CLI relies on; a verb without a route becomes `unavailable` at
  runtime, so it must not ship in a first-party app.
- The test covers `hello` and the eight system apps.

## Decisions

- Routes without CLI verbs are intentional: they serve the app UI, modals and
  asset routes. Parity is CLI → route, not route → CLI.
- The panel's shell routes are not part of the CLI contract. The panel is a
  browser application; the engine and app APIs are the shared boundary.
- The audit command is an engine command, not an app `[[cli]]`; its parity is
  covered by #163's API/CLI limit and cursor tests.

## Out of scope

- Implementing `restart`: the API route is not implemented either, and it is a
  daemon lifecycle change.
- Implementing `anth login/logout` and an operator command: the CLI client is
  Unix-socket-only, so remote sessions need a transport and cookie store.
- Generating Rust/TS clients for the system apps. Their frontends use the
  panel context and hand-written route calls; the generated client stays the
  third-party app surface.
- A compile-time check that route strings in app frontends match manifests.
  The current review used a source scan; a CI script could enforce it later.

## Tests

- `engine/api`: `every_app_cli_verb_has_a_route` (new), plus the golden suite.
- `cargo test -p anth-api`, `fmt` and `clippy` on the touched crate.
