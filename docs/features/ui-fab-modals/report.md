# Report — FAB and modal slots

## Summary

Apps can now contribute a declarative `panel.fab` and a module-backed
`panel.modal`. The FAB supports one action or a menu; an action either opens a
modal (`target`) or sends a local tag to the active view (`action`). The
official panel renders FABs on tabs and floating windows, mounts modals in a
sandboxed dialog, and refreshes the opening view when the modal closes.

## Integrated phases

| Phase | PR | State | Merge |
|-------|----|-------|-------|
| FAB and modal platform | #155 | merged | `fc4222e` |
| Home refresh and system app forms | #156 | merged | `56e834e` |
| Accounts and Files multi-action FABs | #158 | merged | `205b886` |
| FAB/modal QA and documentation | #159 | in review | — |

See [qa.md](qa.md) for the browser matrix.

## Delivered features

- `panel.fab` with `label`, `icon`, `order`, `tag`, optional `view` and nested
  actions; one action runs directly, several open a menu.
- Each action names exactly one destination: a `panel.modal` of the same app
  (`target`) or a local tag delivered to the active view (`action`).
- `panel.modal` modules mount in a sandboxed panel dialog; `context.openModal`,
  `context.close` and `context.onAction` form the bridge.
- After a modal closes, the panel sends the portable `REFRESH` action to the
  view that opened it.
- FAB rendering in tabs and floating windows; menus survive composition polls.
- Home uses the Refresh FAB; Storage, Network, Processes, Firewall, Accounts and
  Files use modals for their create/edit flows.
- Files keeps Upload as a local FAB action; the Editor is unchanged.
- Inline forms remain only as a fallback for panels that do not implement both
  FAB and modal slots.

## Review findings addressed

- FAB `hidden`, re-render signatures, Rust SDK `view`/`actions`, multi-action
  trigger labels and the portable `REFRESH` constant.
- FABs in floating windows, fallbacks for view-only panels, guarded modal
  submits and bundle read errors.
- Files trash actions, Accounts policy fallback, Escape forwarding from modal
  iframes and floating FAB menu Escape.

## Tests

- `engine/`: workspace tests, fmt and clippy; declarative FAB bundle test.
- `sdk/rust/panel`: 7 tests; `sdk/ts/panel`: 13 tests.
- `demo/emulator`: 47 tests, fmt and clippy.
- `panel/frontend`: 14 tests, typecheck and build.
- Apps: typecheck/build for Home, Storage, Network, Processes, Firewall,
  Accounts and Files; manifest validation through the demo bundle builder.
- Headless Chromium QA: desktop, keyboard, Escape/backdrop, floating windows,
  mobile and view-only panel contract tests.

## Recorded decisions

- `panel.fab` is declarative and has no module; `panel.modal` is a normal asset.
- A modal close refreshes the origin view with `REFRESH`; destructive
  confirmations stay local to the app.
- Panels that do not implement FAB/modal ignore those contributions; apps keep a
  local fallback control for portability.

## Out of scope

- App-provided icon assets in the sidebar; tracked as a follow-up.
- A full group floating window; the panel floats the active view only.
- Automated browser tests in CI; QA is a documented manual matrix for now.

## Pending and risks

- Real host validation of the native backends remains manual.
- A panel implementing only one of FAB/modal is not a supported combination for
  the write flows.
