// Stable test hooks for the panel DOM (design §12).
//
// The panel shell, the sandboxed app frame and the e2e suite read the same
// map, so a selector exists once. E2E selects by test id and never by class or
// tag; the classes stay free to change with the layout.
//
// The login and setup pages are server-rendered strings in `panel/backend`; they
// cannot import this module, so their ids are literals there and appear here
// (`login*`, `setup*`) as the shared contract. Everything else is applied with
// `testId()` at the node's creation site.

export const TESTID = {
  // Shell chrome.
  topbar: "topbar",
  brand: "brand",
  health: "health",
  actions: "actions",
  actionButton: "action-button",
  managerButton: "manager-button",
  settingsButton: "settings-button",
  navToggle: "nav-toggle",
  drawerBackdrop: "drawer-backdrop",
  drawerClose: "drawer-close",
  sidebar: "sidebar",
  views: "views",
  viewItem: "view-item",
  viewsPlaceholder: "views-placeholder",
  warnings: "warnings",
  warning: "warning",
  developer: "developer",
  developerToggle: "developer-toggle",
  viewportSelect: "viewport-select",
  viewportEffective: "viewport-effective",
  panelVersion: "panel-version",

  // Workspace.
  workspace: "workspace",
  tabs: "tabs",
  tab: "tab",
  tabSelect: "tab-select",
  tabFloat: "tab-float",
  tabClose: "tab-close",
  panes: "panes",
  emptyState: "empty-state",
  pane: "pane",
  paneHeader: "pane-header",
  paneTitle: "pane-title",
  paneApp: "pane-app",
  paneTools: "pane-tools",
  paneFloat: "pane-float",
  paneClose: "pane-close",
  paneBody: "pane-body",
  paneSubtabs: "pane-subtabs",
  subTab: "sub-tab",

  // App actions.
  fabs: "fabs",
  fab: "fab",
  fabButton: "fab-button",
  fabMenu: "fab-menu",
  fabItem: "fab-item",

  // Floating windows and the minimized stack.
  windows: "windows",
  window: "window",
  windowHeader: "window-header",
  windowTitle: "window-title",
  windowApp: "window-app",
  windowTools: "window-tools",
  windowTool: "window-tool",
  windowBody: "window-body",
  stack: "stack",
  stackItem: "stack-item",
  stackOpen: "stack-open",
  stackClose: "stack-close",

  // Panel-level overlays.
  modal: "modal",
  modalTitle: "modal-title",
  modalApp: "modal-app",
  modalBody: "modal-body",
  modalClose: "modal-close",
  settings: "settings",
  settingsBody: "settings-body",
  settingsClose: "settings-close",
  settingsSection: "settings-section",
  settingsEmpty: "settings-empty",
  manager: "manager",
  managerBody: "manager-body",
  managerClose: "manager-close",
  managerStatus: "manager-status",
  managerApps: "manager-apps",
  managerApp: "manager-app",
  managerCompose: "manager-compose",
  managerComposeItem: "manager-compose-item",
  managerAudit: "manager-audit",

  // Shared module states.
  errorCard: "error-card",
  loading: "loading",

  // The sandboxed frame. `slotRoot` is the frame's `<body>`, where the module
  // mounts; its data attributes name the contribution the frame is showing.
  appFrame: "app-frame",
  slotRoot: "slot-root",

  // Server-rendered auth pages (`panel/backend`).
  loginForm: "login-form",
  loginPassword: "login-password",
  loginSubmit: "login-submit",
  loginStatus: "login-status",
  setupForm: "setup-form",
  setupCurrent: "setup-current",
  setupNew: "setup-new",
  setupSubmit: "setup-submit",
  setupStatus: "setup-status",
} as const;

export type TestId = (typeof TESTID)[keyof typeof TESTID];

/// Marks a node with its stable e2e hook.
export function testId<T extends HTMLElement>(node: T, id: TestId): T {
  node.dataset.testid = id;
  return node;
}
