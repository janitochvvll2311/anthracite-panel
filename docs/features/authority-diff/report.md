# Report — Authority diff

## Summary

The Manager approval screen now compares the frozen grant with the risk the app
reaches in the current composition. It shows the current grant, the effective
risk and any levels that would be new authority, and it refuses to submit an
approval that does not check every required level.

## Delivered

- `GET /api/engine/apps/{id}/approval` returns the risk `approve` computes over
  the **installed** graph and the levels it requires. It works for apps that are
  not running, disabled or degraded.
- `panel/frontend/src/authority.ts`: the risk ladder, `requiredLevels` and
  `authorityDiff`.
- The approval panel fetches the app detail and the approval preview when it
  opens, because the catalogue list deliberately omits the frozen grant.
- The panel shows:
  - current grant risk and who approved it;
  - effective risk from the approval preview;
  - newly required levels, or `none`;
  - one checkbox per level, pre-checked from the grant, with `required` in the
    tooltip where the preview requires it.
- Loading and failure have a Cancel action; a failed preview offers Retry.
- Approve validates the checked set against the preview before calling the
  engine, so the UI does not submit a known-incomplete grant.

## Tests

- `panel/frontend`: **28 tests**, including 10 authority-diff cases.
- Direct `tsc --noEmit` on the touched sources (the package has no typecheck
  script) and `bun run build` clean.
- Browser QA: opening Accounts shows `Current grant admin`, `Effective risk
  admin`, `New authority none`, and the approver metadata.

## Decisions

- The risk comes from the installed graph, via `GET /apps/{id}/approval`,
  matching what `approve` freezes. Apps that are not running, disabled or
  degraded are therefore covered.
- An unknown risk name requires the whole ladder; the UI never under-approves.
- New levels are never pre-checked. Existing accepted levels stay checked, and
  the operator must explicitly add a level the preview now needs.
- The detail and preview fetches are lazy: the list stays one request, and the
  approval panel pays for them only when it opens.

## Out of scope

- Interface-by-interface import/export diff.
- Approval history beyond the grant's `approved_by`/`approved_at`.
- Auto-approval or authority-delta-based silent updates.

## Pending and risks

- Interface-level diff waits for `SurfaceApp` to expose exports/imports (the
  v0.7 platform prerequisite). The current diff is risk-level.
- A preview request that hangs is bounded by the browser/engine session, not by
  the panel; the operator can cancel or retry.
