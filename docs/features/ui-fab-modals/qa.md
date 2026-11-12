# FAB and modal QA

Scope: v0.6.0 phase 4, official panel against the demo composition, plus the
SDK contract tests for a view-only panel.

Environment: Chromium 155 headless, panel on `127.0.0.1:5175`, 8 apps, 0
degraded. The demo bundles were rebuilt with the modal modules and the apps were
upgraded through the engine CLI.

## Matrix

| Case | Result |
| --- | --- |
| Single-action FAB (Home Refresh) | Pass — runs the local action and refreshes Home |
| Multi-action FAB (Accounts Add, Files Add) | Pass — menu opens with every action |
| FAB action `target` (Storage, Network, Processes, Firewall, Accounts) | Pass — opens the matching modal |
| FAB local action (Home refresh, Files upload) | Pass — reaches the active view; Upload opens the file picker |
| Modal close (button, Escape) | Pass — dialog closes and the origin view refreshes |
| Modal backdrop | Pass — pointerdown on the dialog closes it |
| Keyboard menu | Pass — Space on the focused FAB opens the menu; Escape closes it |
| Focus after modal | Pass at panel level — the panel restores the origin frame; the exact element inside the frame stays app-owned |
| Floating window FAB (Home, Storage) | Pass — the window has its own FAB and opens its modal |
| Mobile 390×844 | Pass — FAB stays inside the viewport; modal width 356 px |
| View-only panel | Pass — SDK tests show FAB/MODAL are ignored and `panel.view` still mounts |

## Flows exercised

- Home: FAB Refresh.
- Storage: mount device from the modal and refresh.
- Network: add route from the modal and refresh.
- Processes: spawn from the modal and refresh.
- Firewall: add a rule, then edit it from the row with params.
- Accounts: create a group, set a password from the row.
- Files: create a new file, rename it from the row, and invoke Upload without
  opening a modal.

## Known limits

- Destructive confirmations remain in the app. The panel does not wrap them in a
  composition modal.
- The panel restores focus to the origin frame; an app that wants its internal
  button focused again must do so when it receives `refresh`.
- A panel that implements neither `panel.fab` nor `panel.modal` uses the apps'
  inline fallback forms. A panel that implements one but not the other is not a
  supported combination for these write flows.
