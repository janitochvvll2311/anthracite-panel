# Report — FAB and modal slots

## Summary

Apps can now contribute a declarative `panel.fab` and a module-backed
`panel.modal`. A FAB supports one action or a menu; an action either opens a
modal (`target`) or sends a local tag to the active view (`action`). The
official panel renders FABs on tabs and floating windows, mounts modals in a
sandboxed panel dialog, and sends the portable `refresh` action to the view
that opened the modal when it closes.

## Integrated phases

| Phase | PR | State | Merge |
|-------|----|-------|-------|
| FAB and modal platform | #155 | merged | `fc4222e` |
| Home refresh and system app forms | #156 | merged | `56e834e` |
| Accounts and Files multi-action FABs | #158 | merged | `205b886` |
| FAB/modal QA and documentation | #159 | merged | `628290f` |

See [qa.md](qa.md) for the browser matrix.

## Delivered features

- `panel.fab` with `label`, `icon`, `order`, `tag`, optional `view` and
  nested actions; one action runs directly, several open a menu.
- Each action names exactly one destination: a `panel.modal` of the same app
  (`target`) or a local tag delivered to the active view (`action`).
- `panel.modal` modules mount in a sandboxed panel dialog; `context.openModal`,
  `context.close` and `context.onAction` form the bridge.
- Modal close sends `REFRESH` to the originating view; menus survive
  composition polls and Escape works from tabs, floating windows and modal
  iframes.
- Home uses the Refresh FAB; Storage, Network, Processes, Firewall, Accounts and
  Files use modals for create/edit flows.
- Files keeps Upload as a local FAB action; the Editor is unchanged.
- Inline forms remain only as a fallback for panels that do not implement both
  FAB and modal slots; floating windows carry their own FABs.

## Review findings addressed

- Platform: FAB `hidden`, stable re-render signature, Rust SDK `view`/`actions`,
  multi-action trigger label and portable `REFRESH`.
- Apps: FABs in floating windows, view-only fallbacks, guarded modal submits,
  bundle read errors, Files trash actions and Accounts policy failure handling.
- QA: Escape forwarding from modal iframes, floating FAB menu Escape, contract
  coverage for view-only panels and corrected documentation.

## Tests

- `engine/`: workspace tests, fmt and clippy; declarative FAB bundle test.
- `sdk/rust/panel`: 7 tests; `sdk/ts/panel`: 13 tests.
- `demo/emulator`: 47 tests, fmt and clippy.
- `panel/frontend`: 14 tests and build.
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
