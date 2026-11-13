# Report — Audit log

## Summary

Phase 9 starts with the audit surface: the engine serves the append-only log,
`anth audit` renders it, and the panel Manager shows the same entries with
paging. Approval already lived in the Manager; this closes the audit half of
the `audit and approval UI` item.

## Integrated phases

| Phase | PR | State |
| --- | --- | --- |
| Phase 9 — Conformance, third party and hardening | #163 | open |

## Delivered

- `GET /api/engine/audit?limit=<n>&before=<index>` returns
  `{entries, total, next, limit, before, problems}`.
  - Entries are newest first.
  - `before` defaults to the end of the log; `next` is the value for the next
    older page, or `null`.
  - `limit` defaults to 100 and is capped at 1000.
  - `problems` names unparseable lines instead of hiding the readable history.
- `anth audit [--limit <n>] [--before <n>]` prints one tab-separated row per
  entry and the `next: --before <n>` hint. When the page is partial it prints
  the total to stderr.
- The Manager has an Audit section with actor, action, result, app and detail,
  a refresh button and a `Load older` cursor.
- API and CLI tests cover the page shape, newest-first order and the cursor.

### API and CLI parity

| Capability | API | CLI |
| --- | --- | --- |
| Default page | `limit=100` | `--limit 100` |
| Page size | `limit` 1–1000, `400` otherwise | `--limit` 1–1000, usage error otherwise |
| Older page | `before=<index>` | `--before <index>` |
| Next cursor | `next` | `next: --before <n>` |
| Total | `total` | stderr when the page is partial |
| Unparseable lines | `problems` | `warning:` lines on stderr |
| Order | newest first | newest first |

Both reject `limit=0` and `limit=1001`; neither silently clamps them.

The parity review also checked the fixed engine commands. `status`, `compose`,
`key`, `app` and `audit` each have an API path in the CLI. Two gaps remain
outside this slice: `restart` has no API route or CLI verb yet, and
`login`/`logout` are HTTP session endpoints with no CLI client because the
engine client is Unix-socket-only. `app inspect` stays deferred. Neither gap is
introduced or widened by the audit command.

## Tests

- `engine/api`: **92 unit tests**, including the audit page test, plus the
  golden suite.
- `sdk/rust`: **54 CLI tests**, including parse and request-path coverage.
- `panel/frontend`: **18 tests**, typecheck and build.
- Browser QA: the Manager renders the Audit section, the first page and the
  older page (100 → 200 rows) against a live panel.
- `cargo fmt --check` and `clippy -D warnings` on both Rust workspaces.

## Decisions

- The page is newest first because the operator opens the log to see what just
  happened.
- `before` is an index, not an opaque token: the log is append-only, so an
  index is stable for older pages.
- The endpoint reads through `AuditLog::scan()` so one corrupt line is reported
  in `problems`, not fatal.
- No action/actor filters in this slice. The CLI and Manager can filter once a
  deployment has a log large enough to need it.

## Out of scope

- Export/download, retention and rotation.
- Server-side filters by action, actor, app or result.
- A live tail/stream of audit entries; refresh is explicit.
- A separate audit page outside the Manager.

## Pending and risks

- `scan()` reads the whole log per request. That is acceptable for the first
  slice; a large deployment needs a tail reader or an index before this becomes
  the page path.
- The Manager fetches 100 entries on every refresh; the `Load older` cursor
  keeps the common path cheap.
