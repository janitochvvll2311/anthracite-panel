# Report — Authority diff

## Summary

The Manager approval screen now compares the frozen grant with the risk the app
reaches in the current composition. It shows the current grant, the effective
risk and any levels that would be new authority, and it refuses to submit an
approval that does not check every required level.

## Delivered

- `panel/frontend/src/authority.ts`: the risk ladder, `requiredLevels` and
  `authorityDiff`.
- The approval panel fetches the app detail when it opens, because the catalogue
  list deliberately omits the frozen grant.
- The panel shows:
  - current grant risk and who approved it;
  - effective risk from the composition surface;
  - newly required levels, or `none`;
  - one checkbox per level, pre-checked from the grant, with `required` in the
    tooltip where the effective risk needs it.
- Approve validates the checked set against the effective risk before calling
  the engine, so the UI does not submit a known-incomplete grant.

## Tests

- `panel/frontend`: **26 tests**, including 8 new authority-diff cases.
- Typecheck and build clean.
- Browser QA: opening Accounts shows `Current grant admin`, `Effective risk
  admin`, `New authority none`, and the approver metadata.

## Decisions

- The diff is risk-level, not interface-level. `SurfaceApp` does not carry
  imports/exports yet; that data belongs to a later surface addition.
- New levels are never pre-checked. Existing accepted levels stay checked, and
  the operator must explicitly add a level the effective risk now needs.
- An app that is not running falls back to its grant risk; there is no surface
  risk to compare against.
- The detail fetch is lazy: the list stays one request, and the approval panel
  pays for the grant only when it opens.

## Out of scope

- Interface-by-interface import/export diff.
- Approval history beyond the grant's `approved_by`/`approved_at`.
- Auto-approval or authority-delta-based silent updates.

## Pending and risks

- Interface-level diff waits for `SurfaceApp` to expose exports/imports (the
  v0.7 platform prerequisite).
- A refresh while the approval panel is open reloads the list; the operator may
  need to reopen the panel to fetch the grant again.
