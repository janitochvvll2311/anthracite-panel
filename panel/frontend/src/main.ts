//! The official panel shell.
//!
//! It knows nothing about apps: it reads the composition surface, mounts the
//! modules an app contributes to the portable slots, and keeps the layout
//! in the panel. The engine serves the modules; the panel never touches the
//! registry, the grants or the state tree.

import {
  ACTION,
  FAB,
  MODAL,
  PORTABLE,
  REFRESH,
  SETTINGS,
  VIEW,
  assetUrl,
  byId,
  button,
  contributionKey,
  element,
  errorCard,
  frameContext,
  ignored as sdkIgnored,
  mountFrame,
  ordered as orderContributions,
  safeDispose,
  testId,
  TESTID,
  type App,
  type Contribution,
  type FabAction,
  type FrameContext,
  type FrameHandle,
  type OpenModalRequest,
  type OpenViewRequest,
} from "@anthracite/panel";

import {
  VIEWPORT_KEY,
  initialViewportMode,
  isViewportMode,
  resolveViewport,
  type ResolvedViewport,
  type ViewportMode,
} from "./viewport";
import {
  minimizedFrom,
  toggleMaximized,
  type RestoreMode,
  type WindowMode,
} from "./windows";
import { mountManager } from "./manager";
import { defaultView } from "./defaults";
import { renderIcon } from "./icons";
import { groupFor, groupViews, type ViewGroup } from "./groups";

type DegradedApp = {
  app: string;
  version?: string;
  reason: string;
};

type Surface = {
  apps: App[];
  degraded: DegradedApp[];
  fatal?: { kind?: string; message?: string };
};

type CompositionView = {
  surface: Surface;
};

type Tab = {
  key: string;
  contribution: Contribution;
  params?: unknown;
};

type Mounted = {
  root: HTMLElement;
  contribution: Contribution;
  /// The panel data this mount was opened with.
  params?: unknown;
  dispose: () => void;
  /// Sub-tab strip when this view belongs to a group.
  subtabs?: HTMLElement;
  /// Sends a transient open request to a live frame without remounting it.
  update?: (params: unknown) => void;
  /// Asks the live frame for permission before closing (unsaved changes).
  requestClose?: () => Promise<boolean>;
  /// Reads the current view state before moving it (float/dock).
  snapshot?: () => Promise<unknown>;
  /// Pauses app polling while the pane is not the active tab.
  setVisible?: (visible: boolean) => void;
  /// Delivers a local action tag, with an optional file payload.
  action?: (tag: string, payload?: unknown) => void;
  /// Restores the view's last focused control.
  focus?: () => void;
  /// Last visibility sent to the frame; a refresh must not resend it.
  visible?: boolean;
  /// Set when the host closes or replaces the mount; a late module must not
  /// attach to a detached root.
  cancelled: boolean;
};

type Floating = Mounted & {
  key: string;
  kind: FrameContext["kind"];
  element: HTMLElement;
  mode: WindowMode;
  /// What to restore after a minimize: a maximized window stays maximized.
  restoreMode: RestoreMode;
  /// The geometry captured when the window was maximized.
  bounds?: { left: string; top: string; width: string; height: string };
  maximizeButton: HTMLButtonElement;
  stackItem?: HTMLElement;
  /// FABs live inside the window, not in the workspace overlay.
  fabs?: HTMLElement;
  fabsSignature?: string;
};

/// The single panel-level modal. It hosts one `panel.modal` module in a
/// sandboxed frame and refreshes the view that opened it once it closes.
type ModalState = {
  mount: Mounted;
  /// The tab or window that opened the modal; it receives `refresh` on close.
  originKey: string | null;
  contribution: Contribution;
  /// The element that had focus, restored when the dialog closes.
  trigger: HTMLElement | null;
};

let modalState: ModalState | null = null;

/// Every panel request goes through here: an expired session sends the browser
/// to the login page instead of letting the retry loop spin.
async function request(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response> {
  const response = await fetch(input, init);
  if (response.status === 401) {
    location.assign("/login");
    throw new Error("the session expired");
  }
  return response;
}

const health = byId<HTMLSpanElement>("health");
const views = byId<HTMLElement>("views");
const actions = byId<HTMLElement>("actions");
const warnings = byId<HTMLElement>("warnings");
const tabsElement = byId<HTMLElement>("tabs");
const panes = byId<HTMLElement>("panes");
const empty = byId<HTMLElement>("empty");
const windowsElement = byId<HTMLElement>("windows");
const stackElement = byId<HTMLElement>("stack");
const fabsElement = byId<HTMLElement>("fabs");
const modalElement = byId<HTMLDialogElement>("modal");
const modalTitle = byId<HTMLElement>("modal-title");
const modalApp = byId<HTMLElement>("modal-app");
const modalBody = byId<HTMLElement>("modal-body");
const modalClose = byId<HTMLButtonElement>("modal-close");
const settings = byId<HTMLElement>("settings");
const settingsBody = byId<HTMLElement>("settings-body");
const settingsButton = byId<HTMLButtonElement>("settings-button");
const settingsClose = byId<HTMLButtonElement>("settings-close");
const manager = byId<HTMLElement>("manager");
const managerBody = byId<HTMLElement>("manager-body");
const managerButton = byId<HTMLButtonElement>("manager-button");
const managerClose = byId<HTMLButtonElement>("manager-close");
const viewportSelect = byId<HTMLSelectElement>("viewport-select");
const viewportEffective = byId<HTMLElement>("viewport-effective");
const developer = byId<HTMLDetailsElement>("developer");
const navToggle = byId<HTMLButtonElement>("nav-toggle");
const drawerClose = byId<HTMLButtonElement>("drawer-close");
const drawerBackdrop = byId<HTMLElement>("drawer-backdrop");
const sidebar = byId<HTMLElement>("sidebar");
const workspace = byId<HTMLElement>("workspace");

let surface: Surface | null = null;
let implemented: string[] = [...PORTABLE];
let panelVersion = "0.7.0";
let engineVersion = "";
let versionPending = false;
let activeKey: string | null = null;
let scrolledActiveKey: string | null = null;
let initialViewOpened = false;
let developerTouched = false;
let retry = 0;
let retryTimer: number | undefined;
let zIndex = 10;
const panelVersionElement = document.getElementById("panel-version");
let settingsSignature = "";
let fabsSignature = "";
let managerDispose: (() => void) | null = null;

const tabs = new Map<string, Tab>();
const mounted = new Map<string, Mounted>();
const floating = new Map<string, Floating>();
let settingsMounted: Mounted[] = [];

const requestedViewport = new URLSearchParams(location.search).get("viewport");
const storedViewport = (() => {
  try {
    return localStorage.getItem(VIEWPORT_KEY);
  } catch {
    return null;
  }
})();
let viewportMode: ViewportMode = initialViewportMode(
  requestedViewport,
  storedViewport,
);
let effectiveViewport: ResolvedViewport = "desktop";
let drawerOpen = false;

function storeViewport(mode: ViewportMode): void {
  try {
    localStorage.setItem(VIEWPORT_KEY, mode);
  } catch {
    // Storage can be disabled; the mode still applies for this session.
  }
}

function setDrawer(open: boolean): void {
  if (open && effectiveViewport !== "mobile") return;
  drawerOpen = open;
  document.body.dataset.drawer = open ? "open" : "closed";
  navToggle.setAttribute("aria-expanded", String(open));
  drawerBackdrop.hidden = !open;
  sidebar.inert = effectiveViewport === "mobile" && !open;
  // Keep the menu button usable; inert only the rest of the topbar and the
  // workspace behind the drawer.
  actions.inert = open;
  settingsButton.inert = open;
  workspace.inert = open;
  windowsElement.inert = open;
  if (open) drawerClose.focus();
  else if (effectiveViewport === "mobile") navToggle.focus();
}

function closeDrawer(): void {
  if (drawerOpen) setDrawer(false);
  else if (effectiveViewport === "mobile") sidebar.inert = true;
}

function applyViewport(): void {
  const effective = resolveViewport(viewportMode, window.innerWidth);
  effectiveViewport = effective;
  document.documentElement.dataset.viewport = effective;
  document.documentElement.dataset.viewportMode = viewportMode;
  viewportSelect.value = viewportMode;
  viewportEffective.textContent =
    viewportMode === "auto" ? `auto · ${effective}` : effective;
  viewportSelect.title = `Viewport: ${viewportMode} (${effective})`;
  if (!developerTouched) developer.open = effective !== "mobile";
  if (effective !== "mobile") {
    closeDrawer();
    sidebar.inert = false;
  } else {
    sidebar.inert = !drawerOpen;
  }
}

const ordered = (slot: string): Contribution[] =>
  surface ? orderContributions(surface.apps, slot) : [];

const implementedByPanel = (contribution: Contribution): boolean =>
  implemented.includes(contribution.slot.id);

const allGroups = (): ViewGroup[] =>
  groupViews(ordered(VIEW).filter(implementedByPanel));

const visibleGroups = (): ViewGroup[] =>
  allGroups().filter(
    (group) =>
      implementedByPanel(group.parent) && !group.parent.slot.hidden,
  );

const groupOf = (contribution: Contribution): ViewGroup | undefined =>
  groupFor(allGroups(), contribution);

const visibleChildren = (group: ViewGroup): Contribution[] =>
  group.children.filter(
    (child) => implementedByPanel(child) && !child.slot.hidden,
  );

const openGroups = (): ViewGroup[] =>
  allGroups().filter((group) =>
    group.children.some((child) => tabs.has(contributionKey(child))),
  );

const tabLabel = (group: ViewGroup, child: Contribution): string =>
  contributionKey(child) === contributionKey(group.parent)
    ? child.slot.tab ?? child.slot.label
    : child.slot.label;

const WINDOW_ICONS = {
  dock:
    '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2v7"/><path d="m5 6 3 3 3-3"/><path d="M3 13h10"/></svg>',
  min:
    '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M3 8h10"/></svg>',
  max:
    '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3.5" y="3.5" width="9" height="9" rx="1"/></svg>',
  restore:
    '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"><rect x="5.5" y="5.5" width="7.5" height="7.5" rx="1"/><path d="M3 10.5V4.5A1.5 1.5 0 0 1 4.5 3h6"/></svg>',
  close:
    '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="m4 4 8 8M12 4l-8 8"/></svg>',
} as const;

type WindowIcon = keyof typeof WINDOW_ICONS;

function windowIconButton(
  iconName: WindowIcon,
  title: string,
  label: string,
): HTMLButtonElement {
  const node = button("ghost small", "", title);
  node.dataset.testid = TESTID.windowTool;
  node.innerHTML = WINDOW_ICONS[iconName];
  node.dataset.windowTool = iconName;
  node.setAttribute("aria-label", label);
  return node;
}

function setMaximizeIcon(
  node: HTMLButtonElement,
  maximized: boolean,
): void {
  node.innerHTML = WINDOW_ICONS[maximized ? "restore" : "max"];
  node.title = maximized ? "Restore the window" : "Maximize in the page";
  node.setAttribute(
    "aria-label",
    maximized ? "Restore the window" : "Maximize in the page",
  );
}

function status(kind: "ok" | "warn" | "offline", text: string): void {
  health.dataset.state = kind;
  health.textContent = text;
}

function renderSidebar(): void {
  views.replaceChildren();
  if (!surface) {
    views.append(
      testId(
        element("div", "placeholder", "Reading the composition…"),
        TESTID.viewsPlaceholder,
      ),
    );
    return;
  }

  const groups = visibleGroups();
  if (groups.length === 0) {
    views.append(
      testId(
        element("div", "placeholder", "No app contributes a view."),
        TESTID.viewsPlaceholder,
      ),
    );
    return;
  }

  const label = element("div", "sidebar-title", "Views");
  views.append(label);
  for (const group of groups) {
    const parent = group.parent;
    const key = group.key;
    const node = testId(
      button(
        "view-button",
        "",
        `${parent.slot.label} — ${parent.app.id}`,
      ),
      TESTID.viewItem,
    );
    node.dataset.key = key;
    node.dataset.appId = parent.app.id;
    node.dataset.tag = parent.slot.tag;
    if (group.children.some((child) => contributionKey(child) === activeKey)) {
      node.classList.add("active");
    }

    const badge = element("span", "view-icon");
    const glyph = element("span", "view-glyph");
    const appIcon = parent.app.icon;
    renderIcon(glyph, {
      appId: parent.app.id,
      icon: appIcon ?? parent.slot.icon,
      icon_sha256: appIcon ? parent.app.icon_sha256 : parent.slot.icon_sha256,
      fallback: parent.slot.label,
    });
    badge.append(glyph);
    const copy = element("span", "view-copy");
    copy.append(
      element("span", "view-label", parent.slot.label),
      element("span", "view-app", parent.app.id),
    );
    node.append(badge, copy);
    node.addEventListener("click", () => openGroup(group));
    views.append(node);
  }
}

function renderActions(): void {
  actions.replaceChildren();
  if (!surface) return;
  for (const contribution of ordered(ACTION).filter(
    (candidate) => implementedByPanel(candidate) && !candidate.slot.hidden,
  )) {
    const node = testId(
      button(
        "action-button",
        contribution.slot.label,
        `${contribution.slot.label} — ${contribution.app.id}`,
      ),
      TESTID.actionButton,
    );
    node.dataset.appId = contribution.app.id;
    const badge = element("span", "action-icon", "⚡");
    node.prepend(badge);
    node.addEventListener("click", () => {
      closeDrawer();
      openWindow(contribution, "action");
    });
    actions.append(node);
  }
}

function renderWarnings(): void {
  warnings.replaceChildren();
  if (!surface) return;

  if (surface.fatal) {
    const node = testId(
      element(
        "div",
        "warning fatal",
        `The composition cannot boot: ${surface.fatal.message ?? "unknown error"}`,
      ),
      TESTID.warning,
    );
    node.dataset.kind = "fatal";
    warnings.append(node);
  }
  for (const degraded of surface.degraded) {
    const node = testId(
      element("div", "warning", `${degraded.app}: ${degraded.reason}`),
      TESTID.warning,
    );
    node.dataset.kind = "degraded";
    node.dataset.appId = degraded.app;
    warnings.append(node);
  }
  const ignored = sdkIgnored(surface.apps, implemented);
  if (ignored.length > 0) {
    const node = testId(
      element(
        "div",
        "warning muted-warning",
        `${ignored.length} contribution(s) target slots this panel does not implement.`,
      ),
      TESTID.warning,
    );
    node.dataset.kind = "ignored";
    warnings.append(node);
  }
}

function renderTabs(): void {
  tabsElement.replaceChildren();
  const groups = openGroups();
  for (const group of groups) {
    const active = group.children.some(
      (child) => contributionKey(child) === activeKey,
    );
    const node = testId(element("div", "tab"), TESTID.tab);
    node.dataset.key = group.key;
    node.dataset.appId = group.parent.app.id;
    node.dataset.tag = group.parent.slot.tag;
    node.setAttribute("role", "tab");
    if (active) {
      node.classList.add("active");
      node.setAttribute("aria-selected", "true");
    }

    const select = testId(
      button("tab-label", group.parent.slot.label),
      TESTID.tabSelect,
    );
    select.title = group.parent.app.id;
    select.addEventListener("click", () => activateGroup(group));

    const detach = testId(
      button("tab-tool tab-float", "⇱", "Float the active view"),
      TESTID.tabFloat,
    );
    detach.addEventListener("click", (event) => {
      event.stopPropagation();
      detachGroup(group);
    });

    const close = testId(
      button("tab-tool", "×", "Close this group"),
      TESTID.tabClose,
    );
    close.addEventListener("click", (event) => {
      event.stopPropagation();
      void requestGroupClose(group);
    });

    node.append(select, detach, close);
    tabsElement.append(node);
    if (active && activeKey !== scrolledActiveKey) {
      scrolledActiveKey = activeKey;
      node.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }
  if (activeKey === null) scrolledActiveKey = null;
  tabsElement.hidden = groups.length === 0;
  workspace.classList.toggle("no-tabs", groups.length === 0);
}

async function mountInto(
  root: HTMLElement,
  contribution: Contribution,
  kind: FrameContext["kind"],
  close: () => void,
  cancelled: () => boolean,
  params?: unknown,
  openView?: (request: OpenViewRequest) => boolean | void,
  openModal?: (request: OpenModalRequest) => boolean | void,
  onParams?: (params: unknown) => void,
  onReady?: (frame: FrameHandle) => void,
): Promise<() => void> {
  root.replaceChildren(testId(element("div", "loading", "Loading module…"), TESTID.loading));
  try {
    const response = await request(assetUrl(contribution), {
      headers: { accept: "text/javascript" },
    });
    if (!response.ok) throw new Error(await errorMessage(response));
    const source = await response.text();
    if (cancelled()) return () => root.replaceChildren();
    const dispose = await mountFrame(root, {
      source,
      context: frameContext(
        contribution,
        kind,
        {
          slots: implemented,
          version: panelVersion,
        },
        params,
      ),
      close,
      openView,
      openModal,
      onParams,
      onReady,
      request: (input, init) => request(input, init),
    });
    if (cancelled()) {
      dispose();
      return () => root.replaceChildren();
    }
    return dispose;
  } catch (error) {
    if (!cancelled()) root.replaceChildren(errorCard(error));
    return () => root.replaceChildren();
  }
}

function retire(state: Mounted): void {
  state.cancelled = true;
  safeDispose(state.dispose);
  state.root.parentElement?.remove();
}

function retireFloating(state: Floating): void {
  state.cancelled = true;
  safeDispose(state.dispose);
  state.element.remove();
  state.stackItem?.remove();
  state.stackItem = undefined;
}

function trackMount(
  state: Mounted,
  body: HTMLElement,
  contribution: Contribution,
  kind: FrameContext["kind"],
  close: () => void,
  current: () => Mounted | undefined,
  params?: unknown,
  openView?: (request: OpenViewRequest) => boolean | void,
  openModal?: (request: OpenModalRequest) => boolean | void,
  onParams?: (params: unknown) => void,
): void {
  void mountInto(
    body,
    contribution,
    kind,
    close,
    () => state.cancelled,
    params,
    openView,
    openModal,
    onParams,
    (frame) => {
      state.update = frame.update;
      state.requestClose = frame.requestClose;
      state.snapshot = frame.snapshot;
      state.setVisible = frame.setVisible;
      state.action = frame.action;
      state.focus = frame.focus;
      const pane = state.root.closest(".pane");
      const visible = !pane || pane.classList.contains("active");
      state.visible = visible;
      frame.setVisible(visible);
    },
  ).then((dispose) => {
    if (state.cancelled || current() !== state) {
      safeDispose(dispose);
      return;
    }
    state.dispose = dispose;
  });
}

function lastTabKey(): string | null {
  return tabs.size > 0 ? [...tabs.keys()][tabs.size - 1] : null;
}

function renderWorkspace(): void {
  renderSidebar();
  renderTabs();
  renderPanes();
  renderFabs();
}

/// Sends a local action tag to a live view, whether it is a tab or a window.
function sendAction(
  key: string | null,
  tag: string,
  payload?: unknown,
): void {
  if (!key) return;
  const state = mounted.get(key) ?? floating.get(key);
  state?.action?.(tag, payload);
}

/// Opens the picker synchronously from the FAB click, reads the files and
/// sends them with the local action. Doing this in the panel preserves the
/// user activation that a postMessage round-trip would lose.
function pickFiles(originKey: string | null, tag: string): void {
  if (!originKey) return;
  const input = document.createElement("input");
  input.type = "file";
  input.multiple = true;
  input.hidden = true;
  document.body.append(input);
  input.addEventListener(
    "change",
    () => {
      const files = [...(input.files ?? [])];
      input.remove();
      if (files.length > 0) sendAction(originKey, tag, { files });
    },
    { once: true },
  );
  input.addEventListener("cancel", () => input.remove(), { once: true });
  input.click();
}

function closeFabMenus(): void {
  for (const menu of document.querySelectorAll<HTMLElement>(".fab-menu")) {
    menu.hidden = true;
  }
  for (const trigger of document.querySelectorAll<HTMLElement>(
    ".fab-button[aria-expanded]",
  )) {
    trigger.setAttribute("aria-expanded", "false");
  }
}

function fabButton(
  className: string,
  label: string,
  appId: string,
  iconName?: string,
  iconSha256?: string,
  iconOnly = false,
): HTMLButtonElement {
  const node = button(className, "", label);
  const hasIcon = Boolean(iconName?.trim() || iconSha256);
  const onlyIcon = iconOnly && hasIcon;
  if (onlyIcon) node.classList.add("fab-icon-only");
  const glyph = element("span", "fab-icon");
  renderIcon(glyph, {
    appId,
    icon: iconName,
    icon_sha256: iconSha256,
    fallback: label,
  });
  node.append(glyph);
  if (!onlyIcon) node.append(element("span", "fab-label", label));
  node.setAttribute("aria-label", label);
  node.title = label;
  return node;
}

function runFabAction(
  source: Contribution,
  action: FabAction,
  originKey: string | null,
  trigger?: HTMLElement,
): void {
  closeFabMenus();
  if (action.target) {
    openModal(source, { tag: action.target }, originKey, trigger);
    return;
  }
  if (!action.action) return;
  if (action.files) pickFiles(originKey, action.action);
  else sendAction(originKey, action.action);
}

function renderFab(
  source: Contribution,
  fab: Contribution,
  originKey: string | null,
): HTMLElement {
  const actions = fab.slot.actions ?? [];
  const wrap = testId(element("div", "fab"), TESTID.fab);
  wrap.dataset.tag = fab.slot.tag;
  if (actions.length <= 1) {
    const action = actions[0];
    if (action) {
      const node = testId(
        fabButton(
          "fab-button",
          action.label,
          source.app.id,
          action.icon ?? fab.slot.icon,
          action.icon ? action.icon_sha256 : fab.slot.icon_sha256,
          true,
        ),
        TESTID.fabButton,
      );
      node.addEventListener("click", () =>
        runFabAction(source, action, originKey, node),
      );
      wrap.append(node);
    }
    return wrap;
  }

  const menu = testId(element("div", "fab-menu"), TESTID.fabMenu);
  menu.hidden = true;
  menu.setAttribute("role", "menu");
  let trigger: HTMLElement | null = null;
  for (const action of actions) {
    const item = testId(
      fabButton(
        "fab-menu-item",
        action.label,
        source.app.id,
        action.icon,
        action.icon_sha256,
      ),
      TESTID.fabItem,
    );
    item.dataset.tag = action.tag;
    item.setAttribute("role", "menuitem");
    item.addEventListener("click", () =>
      runFabAction(source, action, originKey, trigger ?? item),
    );
    menu.append(item);
  }
  const main = testId(
    fabButton(
      "fab-button fab-multi",
      fab.slot.label,
      source.app.id,
      fab.slot.icon,
      fab.slot.icon_sha256,
      true,
    ),
    TESTID.fabButton,
  );
  trigger = main;
  main.setAttribute("aria-haspopup", "menu");
  main.setAttribute("aria-expanded", "false");
  main.addEventListener("click", () => {
    const open = menu.hidden;
    closeFabMenus();
    menu.hidden = !open;
    main.setAttribute("aria-expanded", String(open));
  });
  wrap.append(menu, main);
  return wrap;
}

/// FABs belong to the app and optionally to one of its views. Tabs and
/// floating windows each carry their own FAB container.
///
/// The composition is polled every few seconds, so this must not rebuild the
/// DOM when nothing changed: a `replaceChildren` would close an open menu and
/// drop keyboard focus while the operator is using it.
function fabsFor(contribution: Contribution): Contribution[] {
  return ordered(FAB).filter(
    (candidate) =>
      implementedByPanel(candidate) &&
      !candidate.slot.hidden &&
      candidate.app.id === contribution.app.id &&
      (!candidate.slot.view || candidate.slot.view === contribution.slot.tag),
  );
}

function fabsSignatureFor(
  scope: string | null,
  enabled: boolean,
  fabs: Contribution[],
): string {
  return JSON.stringify([
    scope,
    enabled,
    fabs.map((fab) => [
      fab.slot.tag,
      fab.slot.label,
      fab.slot.icon ?? null,
      fab.slot.icon_sha256 ?? null,
      fab.slot.order,
      fab.slot.hidden === true,
      fab.slot.view ?? null,
      (fab.slot.actions ?? []).map((action) => [
        action.label,
        action.icon ?? null,
        action.icon_sha256 ?? null,
        action.tag,
        action.target ?? null,
        action.action ?? null,
        action.files === true,
      ]),
    ]),
  ]);
}

function renderFabs(): void {
  const active = activeKey ? tabs.get(activeKey)?.contribution : undefined;
  const enabled = Boolean(active) && implemented.includes(FAB);
  const fabs = enabled && active ? fabsFor(active) : [];
  const signature = fabsSignatureFor(
    active ? activeKey : null,
    enabled,
    fabs,
  );
  if (signature === fabsSignature) return;
  fabsSignature = signature;

  fabsElement.replaceChildren();
  if (!enabled || !active) {
    fabsElement.hidden = true;
    return;
  }
  fabsElement.hidden = fabs.length === 0;
  for (const fab of fabs) fabsElement.append(renderFab(active, fab, activeKey));
  closeFabMenus();
}

function renderWindowFabs(state: Floating): void {
  const container = state.fabs;
  if (!container) return;
  const enabled = implemented.includes(FAB);
  const fabs = enabled ? fabsFor(state.contribution) : [];
  const signature = fabsSignatureFor(state.key, enabled, fabs);
  if (signature === state.fabsSignature) return;
  state.fabsSignature = signature;
  container.replaceChildren();
  container.hidden = fabs.length === 0;
  for (const fab of fabs) {
    container.append(renderFab(state.contribution, fab, state.key));
  }
  closeFabMenus();
}

/// Opens one of the app's modals in the panel-level dialog. Only one modal is
/// open at a time; the view that opened it gets a `refresh` action on close.
function openModal(
  source: Contribution,
  request: OpenModalRequest,
  originKey: string | null = activeKey,
  trigger?: HTMLElement,
): boolean {
  if (!implemented.includes(MODAL) || modalState) return false;
  const target = ordered(MODAL).find(
    (candidate) =>
      implementedByPanel(candidate) &&
      candidate.app.id === source.app.id &&
      candidate.slot.tag === request.tag,
  );
  if (!target) return false;

  modalTitle.textContent = target.slot.label;
  modalApp.textContent = target.app.id;
  modalBody.replaceChildren(
    testId(element("div", "loading", "Loading modal…"), TESTID.loading),
  );
  const mount: Mounted = {
    root: modalBody,
    contribution: target,
    params: request.params,
    dispose: () => modalBody.replaceChildren(),
    cancelled: false,
  };
  const active = trigger ?? document.activeElement;
  const origin =
    active instanceof HTMLElement && active !== document.body ? active : null;
  modalState = { mount, originKey, contribution: target, trigger: origin };
  modalElement.showModal();
  trackMount(
    mount,
    modalBody,
    target,
    "modal",
    () => closeModal(),
    () => modalState?.mount,
    request.params,
    (open) => openAppView(target, open),
    (open) => openModal(target, open, originKey),
    undefined,
  );
  return true;
}

function closeModal(refresh = true): void {
  const state = modalState;
  if (!state) {
    if (modalElement.open) modalElement.close();
    return;
  }
  modalState = null;
  state.mount.cancelled = true;
  safeDispose(state.mount.dispose);
  modalBody.replaceChildren();
  if (modalElement.open) modalElement.close();
  modalTitle.textContent = "";
  modalApp.textContent = "";
  if (refresh) sendAction(state.originKey, REFRESH);
  const origin = state.originKey
    ? mounted.get(state.originKey) ?? floating.get(state.originKey)
    : undefined;
  if (state.trigger?.tagName === "IFRAME") {
    const frame = state.trigger as HTMLIFrameElement;
    window.setTimeout(() => {
      frame.focus();
      origin?.focus?.();
    }, 0);
  } else if (state.trigger?.isConnected) {
    state.trigger.focus();
  }
}

function paramsKey(params: unknown): string {
  return JSON.stringify(params ?? null);
}

function rememberParams(contribution: Contribution, params: unknown): void {
  const key = contributionKey(contribution);
  const tab = tabs.get(key);
  if (tab) tab.params = params;
  const pane = mounted.get(key);
  if (pane) pane.params = params;
  for (const state of floating.values()) {
    if (contributionKey(state.contribution) === key) state.params = params;
  }
}

function openAppView(
  source: Contribution,
  request: OpenViewRequest,
): boolean {
  const target = ordered(VIEW).find(
    (contribution) =>
      contribution.app.id === source.app.id &&
      contribution.slot.tag === request.tag,
  );
  if (!target) return false;
  const key = contributionKey(target);
  const windowKey = `window:view:${key}`;
  const windowState = floating.get(windowKey);
  if (windowState) {
    if (windowState.mode === "minimized") restoreStack(windowState);
    else focusWindow(windowState);
    windowState.update?.(request.params);
    closeDrawer();
    return true;
  }
  const tab = tabs.get(key);
  if (tab) {
    activate(key);
    mounted.get(key)?.update?.(request.params);
    renderWorkspace();
    closeDrawer();
    return true;
  }
  openWindow(target, "view", request.params, request.size);
  return true;
}

function renderSubtabs(state: Mounted): void {
  const group = groupOf(state.contribution);
  const children = group ? visibleChildren(group) : [];
  if (!group || children.length < 2) {
    state.subtabs?.remove();
    state.subtabs = undefined;
    return;
  }
  if (!state.subtabs) {
    const pane = state.root.parentElement;
    if (!pane) return;
    const bar = testId(element("div", "pane-subtabs"), TESTID.paneSubtabs);
    pane.insertBefore(bar, pane.firstChild);
    state.subtabs = bar;
  }
  state.subtabs.replaceChildren();
  for (const child of children) {
    const key = contributionKey(child);
    const node = testId(button("sub-tab", "", child.app.id), TESTID.subTab);
    node.dataset.key = key;
    node.dataset.appId = child.app.id;
    const glyph = element("span", "sub-tab-icon");
    const appIcon = child.app.icon;
    renderIcon(glyph, {
      appId: child.app.id,
      icon: appIcon ?? child.slot.icon,
      icon_sha256: appIcon ? child.app.icon_sha256 : child.slot.icon_sha256,
      fallback: child.slot.label,
    });
    node.append(
      glyph,
      element("span", "sub-tab-label", tabLabel(group, child)),
    );
    if (key === activeKey) node.classList.add("active");
    node.addEventListener("click", () => openTab(child));
    state.subtabs.append(node);
  }
}

function ensurePane(contribution: Contribution, params?: unknown): void {
  const key = contributionKey(contribution);
  const existing = mounted.get(key);
  if (existing) {
    if (
      existing.contribution.slot.sha256 !== contribution.slot.sha256 ||
      paramsKey(existing.params) !== paramsKey(params)
    ) {
      retire(existing);
      mounted.delete(key);
    } else {
      existing.contribution = contribution;
      return;
    }
  }

  const pane = testId(element("section", "pane"), TESTID.pane);
  pane.dataset.key = key;
  pane.dataset.appId = contribution.app.id;
  pane.dataset.tag = contribution.slot.tag;
  pane.dataset.active = "false";

  const head = testId(element("header", "pane-head"), TESTID.paneHeader);
  const copy = element("div", "pane-copy");
  copy.append(
    testId(
      element("strong", "", contribution.slot.label),
      TESTID.paneTitle,
    ),
    testId(element("span", "pane-app", contribution.app.id), TESTID.paneApp),
  );
  const tools = testId(element("div", "pane-tools"), TESTID.paneTools);
  const detach = testId(
    button("ghost small", "Float", "Open this view in a floating window"),
    TESTID.paneFloat,
  );
  detach.addEventListener("click", () => {
    const group = groupOf(contribution);
    if (group) detachGroup(group);
    else void detachTab(key);
  });
  const close = testId(
    button("ghost small", "Close", "Close this view group"),
    TESTID.paneClose,
  );
  close.addEventListener("click", () => {
    const group = groupOf(contribution);
    if (group) void requestGroupClose(group);
    else void requestTabClose(key);
  });
  tools.append(detach, close);
  head.append(copy, tools);

  const body = testId(element("div", "pane-body"), TESTID.paneBody);
  pane.append(head, body);
  panes.append(pane);

  const state: Mounted = {
    root: body,
    contribution,
    params,
    dispose: () => body.replaceChildren(),
    cancelled: false,
  };
  mounted.set(key, state);
  trackMount(
    state,
    body,
    contribution,
    "view",
    () => closeTab(key),
    () => mounted.get(key),
    params,
    (request) => openAppView(contribution, request),
    (request) => openModal(contribution, request, key),
    (next) => rememberParams(contribution, next),
  );
}

function renderPanes(): void {
  for (const [key, state] of mounted) {
    if (!tabs.has(key)) {
      retire(state);
      mounted.delete(key);
    }
  }
  for (const tab of tabs.values()) ensurePane(tab.contribution, tab.params);
  for (const [key, state] of mounted) {
    if (tabs.has(key)) renderSubtabs(state);
    else {
      state.subtabs?.remove();
      state.subtabs = undefined;
    }
  }
  for (const [key, state] of mounted) {
    const pane = state.root.parentElement;
    if (!pane) continue;
    const visible = key === activeKey;
    pane.classList.toggle("active", visible);
    pane.dataset.active = String(visible);
    if (state.visible === visible) continue;
    state.visible = visible;
    state.setVisible?.(visible);
  }
  empty.classList.toggle("hidden", tabs.size > 0);
}

function openTab(
  contribution: Contribution,
  activateTab = true,
  params?: unknown,
): void {
  openSingle(contribution, activateTab, params);
}

function openSingle(
  contribution: Contribution,
  activateTab: boolean,
  params?: unknown,
): void {
  const key = contributionKey(contribution);
  // A view that is already floating (or minimized) comes forward instead of
  // mounting a second copy as a tab.
  const detached = floating.get(`window:view:${key}`);
  if (detached) {
    if (paramsKey(detached.params) !== paramsKey(params)) {
      closeFloating(detached);
    } else {
      if (detached.mode === "minimized") restoreStack(detached);
      else focusWindow(detached);
      closeDrawer();
      return;
    }
  }
  const existing = tabs.get(key);
  if (existing) {
    existing.contribution = contribution;
    if (params !== undefined) existing.params = params;
  } else {
    tabs.set(key, { key, contribution, params });
  }
  if (activateTab) activate(key);
  renderWorkspace();
  closeDrawer();
}

function preferredChild(group: ViewGroup): Contribution | undefined {
  const current = activeKey;
  const active =
    current === null
      ? undefined
      : group.children.find(
          (child) =>
            contributionKey(child) === current && tabs.has(current),
        );
  return (
    active ??
    group.children.find((child) => tabs.has(contributionKey(child)))
  );
}

function openGroup(group: ViewGroup): void {
  const child =
    preferredChild(group) ??
    visibleChildren(group)[0] ??
    group.parent;
  openTab(child);
}

function activate(key: string): void {
  activeKey = key;
  renderWorkspace();
}

function activateGroup(group: ViewGroup): void {
  openTab(preferredChild(group) ?? group.parent);
}

function closeTab(key: string, render = true): void {
  if (modalState?.originKey === key) closeModal(false);
  const state = mounted.get(key);
  if (state) {
    retire(state);
    mounted.delete(key);
  }
  tabs.delete(key);
  if (activeKey === key) {
    activeKey = lastTabKey();
  }
  if (render) renderWorkspace();
}

function closeGroup(group: ViewGroup, render = true): void {
  for (const child of group.children) {
    const key = contributionKey(child);
    if (tabs.has(key)) closeTab(key, false);
  }
  if (render) renderWorkspace();
}

async function allowClose(state: Mounted | undefined): Promise<boolean> {
  if (!state?.requestClose) return true;
  try {
    return await state.requestClose();
  } catch {
    return false;
  }
}

async function requestTabClose(key: string): Promise<void> {
  if (!(await allowClose(mounted.get(key)))) return;
  closeTab(key);
}

async function requestGroupClose(group: ViewGroup): Promise<void> {
  for (const child of group.children) {
    const key = contributionKey(child);
    if (!tabs.has(key)) continue;
    if (!(await allowClose(mounted.get(key)))) return;
  }
  closeGroup(group);
}

function detachGroup(group: ViewGroup): void {
  const child = preferredChild(group);
  if (child) void detachTab(contributionKey(child));
}

async function detachTab(key: string): Promise<void> {
  const tab = tabs.get(key);
  if (!tab) return;
  const params = await latestParams(mounted.get(key), tab.params);
  if (!tabs.has(key)) return;
  closeDrawer();
  openWindow(tab.contribution, "view", params);
  closeTab(key);
}

function focusWindow(state: Floating): void {
  if (state.mode === "minimized") return;
  state.element.style.zIndex = String(++zIndex);
}

function updateStack(): void {
  const minimized = [...floating.values()].filter(
    (state) => state.mode === "minimized",
  ).length;
  stackElement.hidden = minimized === 0;
}

function closeFloating(state: Floating): void {
  if (modalState?.originKey === state.key) closeModal(false);
  retireFloating(state);
  floating.delete(state.key);
  updateStack();
}

async function requestWindowClose(state: Floating): Promise<void> {
  if (!(await allowClose(state))) return;
  closeFloating(state);
}

async function latestParams(
  state: Mounted | undefined,
  fallback: unknown,
): Promise<unknown> {
  if (!state?.snapshot) return fallback;
  try {
    const value = await state.snapshot();
    return value ?? fallback;
  } catch {
    return fallback;
  }
}

function setMaximized(state: Floating, maximized: boolean): void {
  if (maximized) {
    if (state.mode === "maximized") return;
    state.bounds = {
      left: state.element.style.left || `${state.element.offsetLeft}px`,
      top: state.element.style.top || `${state.element.offsetTop}px`,
      width: `${state.element.offsetWidth}px`,
      height: `${state.element.offsetHeight}px`,
    };
    state.element.style.left = "0px";
    state.element.style.top = "0px";
    state.element.style.right = "0px";
    state.element.style.bottom = "0px";
    state.element.style.width = "auto";
    state.element.style.height = "auto";
    state.element.classList.add("maximized");
    state.mode = "maximized";
    state.element.dataset.mode = "maximized";
    setMaximizeIcon(state.maximizeButton, true);
    focusWindow(state);
    return;
  }
  state.element.classList.remove("maximized");
  state.element.style.right = "";
  state.element.style.bottom = "";
  if (state.bounds) {
    state.element.style.left = state.bounds.left;
    state.element.style.top = state.bounds.top;
    state.element.style.width = state.bounds.width;
    state.element.style.height = state.bounds.height;
  }
  state.mode = "normal";
  state.element.dataset.mode = "normal";
  setMaximizeIcon(state.maximizeButton, false);
}

function minimizeWindow(state: Floating): void {
  if (state.mode === "minimized") return;
  state.restoreMode = minimizedFrom(state.mode);
  state.mode = "minimized";
  state.element.classList.add("minimized");
  state.element.dataset.mode = "minimized";

  const item = testId(element("div", "stack-item"), TESTID.stackItem);
  item.dataset.key = state.key;
  item.dataset.appId = state.contribution.app.id;
  item.dataset.tag = state.contribution.slot.tag;
  const open = testId(button("stack-open", "", ""), TESTID.stackOpen);
  open.title = `Restore ${state.contribution.slot.label}`;
  open.append(
    element("span", "stack-label", state.contribution.slot.label),
    element("span", "stack-app", state.contribution.app.id),
  );
  open.addEventListener("click", () => restoreStack(state));
  const close = testId(button("stack-close", "×", "Close"), TESTID.stackClose);
  close.addEventListener("click", (event) => {
    event.stopPropagation();
    void requestWindowClose(state);
  });
  item.append(open, close);
  state.stackItem = item;
  stackElement.append(item);
  updateStack();
}

function restoreStack(state: Floating): void {
  if (state.mode !== "minimized") return;
  state.stackItem?.remove();
  state.stackItem = undefined;
  state.element.classList.remove("minimized");
  if (state.restoreMode === "maximized") {
    state.element.classList.add("maximized");
    state.mode = "maximized";
    setMaximizeIcon(state.maximizeButton, true);
  } else {
    state.element.classList.remove("maximized");
    state.mode = "normal";
    setMaximizeIcon(state.maximizeButton, false);
  }
  state.element.dataset.mode = state.mode;
  updateStack();
  focusWindow(state);
}

async function dockWindow(state: Floating): Promise<void> {
  if (state.kind !== "view") return;
  const params = await latestParams(state, state.params);
  if (!floating.has(state.key)) return;
  const contribution = state.contribution;
  closeFloating(state);
  openTab(contribution, true, params);
}

function openWindow(
  contribution: Contribution,
  kind: FrameContext["kind"],
  params?: unknown,
  size?: { width?: number; height?: number },
): void {
  const key = `window:${kind}:${contributionKey(contribution)}`;
  const existing = floating.get(key);
  if (existing) {
    if (params === undefined) params = existing.params;
    if (
      existing.contribution.slot.sha256 === contribution.slot.sha256 &&
      paramsKey(existing.params) === paramsKey(params)
    ) {
      if (existing.mode === "minimized") restoreStack(existing);
      else focusWindow(existing);
      return;
    }
    closeFloating(existing);
  }

  const width = Math.max(
    320,
    Math.min(size?.width ?? 480, window.innerWidth - 24),
  );
  const height = Math.max(
    220,
    Math.min(size?.height ?? 340, window.innerHeight - 24),
  );
  const offset = 74 + floating.size * 28;
  const windowElement = testId(element("section", "window"), TESTID.window);
  windowElement.dataset.key = key;
  windowElement.dataset.kind = kind;
  windowElement.dataset.appId = contribution.app.id;
  windowElement.dataset.tag = contribution.slot.tag;
  windowElement.dataset.mode = "normal";
  windowElement.style.width = `${width}px`;
  windowElement.style.height = `${height}px`;
  windowElement.style.left = `${Math.min(offset, window.innerWidth - width - 12)}px`;
  windowElement.style.top = `${Math.min(offset, window.innerHeight - height - 12)}px`;

  const header = testId(element("header", "window-head"), TESTID.windowHeader);
  const copy = element("div", "window-copy");
  copy.append(
    testId(
      element("strong", "", contribution.slot.label),
      TESTID.windowTitle,
    ),
    testId(
      element("span", "window-app", contribution.app.id),
      TESTID.windowApp,
    ),
  );
  const tools = testId(element("div", "window-tools"), TESTID.windowTools);
  const dock = windowIconButton(
    "dock",
    "Dock this view back into the tabs",
    "Dock",
  );
  dock.hidden = kind !== "view";
  const minimize = windowIconButton(
    "min",
    "Minimize to the bottom stack",
    "Minimize",
  );
  const maximize = windowIconButton(
    "max",
    "Maximize in the page",
    "Maximize",
  );
  const close = windowIconButton("close", "Close", "Close");
  tools.append(dock, minimize, maximize, close);
  header.append(copy, tools);

  const body = testId(element("div", "window-body"), TESTID.windowBody);
  const fabs = element("div", "fabs window-fabs");
  fabs.hidden = true;
  windowElement.append(header, body, fabs);
  windowsElement.append(windowElement);

  const state: Floating = {
    key,
    kind,
    element: windowElement,
    root: body,
    contribution,
    params,
    dispose: () => body.replaceChildren(),
    cancelled: false,
    mode: "normal",
    restoreMode: "normal",
    maximizeButton: maximize,
    fabs,
  };
  floating.set(key, state);
  focusWindow(state);
  renderWindowFabs(state);

  maximize.addEventListener("click", () =>
    setMaximized(state, toggleMaximized(state.mode) === "maximized"),
  );
  minimize.addEventListener("click", () => minimizeWindow(state));
  dock.addEventListener("click", () => {
    void dockWindow(state);
  });
  close.addEventListener("click", () => {
    void requestWindowClose(state);
  });
  windowElement.addEventListener("pointerdown", () => focusWindow(state));
  makeDraggable(windowElement, header);
  trackMount(
    state,
    body,
    contribution,
    kind,
    () => closeFloating(state),
    () => floating.get(key),
    params,
    kind === "view"
      ? (request) => openAppView(contribution, request)
      : undefined,
    kind === "view"
      ? (request) => openModal(contribution, request, key)
      : undefined,
    (next) => rememberParams(contribution, next),
  );
}

function makeDraggable(elementNode: HTMLElement, handle: HTMLElement): void {
  let originX = 0;
  let originY = 0;
  let pointerId = -1;

  handle.addEventListener("pointerdown", (event) => {
    if (document.documentElement.dataset.viewport === "mobile") return;
    if (elementNode.classList.contains("maximized")) return;
    if ((event.target as HTMLElement).closest("button")) return;
    pointerId = event.pointerId;
    originX = event.clientX - elementNode.offsetLeft;
    originY = event.clientY - elementNode.offsetTop;
    handle.setPointerCapture(pointerId);
    elementNode.classList.add("dragging");
  });

  handle.addEventListener("pointermove", (event) => {
    if (pointerId !== event.pointerId) return;
    const left = Math.min(
      Math.max(0, event.clientX - originX),
      Math.max(0, window.innerWidth - 220),
    );
    const top = Math.min(
      Math.max(0, event.clientY - originY),
      Math.max(0, window.innerHeight - 60),
    );
    elementNode.style.left = `${left}px`;
    elementNode.style.top = `${top}px`;
  });

  const stop = (event: PointerEvent): void => {
    if (pointerId !== event.pointerId) return;
    pointerId = -1;
    elementNode.classList.remove("dragging");
  };
  handle.addEventListener("pointerup", stop);
  handle.addEventListener("pointercancel", stop);
}

function disposeSettings(): void {
  for (const state of settingsMounted) {
    retire(state);
  }
  settingsMounted = [];
}

function renderSettings(): void {
  const contributions = ordered(SETTINGS).filter(
    (contribution) => implementedByPanel(contribution) && !contribution.slot.hidden,
  );
  const signature = contributions
    .map((contribution) => `${contributionKey(contribution)}:${contribution.slot.sha256}`)
    .join("|");
  if (signature === settingsSignature && settingsBody.childElementCount > 0) {
    return;
  }
  settingsSignature = signature;
  disposeSettings();
  settingsBody.replaceChildren();
  if (contributions.length === 0) {
    settingsBody.append(
      testId(
        element("p", "placeholder", "No app contributes settings."),
        TESTID.settingsEmpty,
      ),
    );
    return;
  }
  for (const contribution of contributions) {
    const section = testId(
      element("section", "settings-section"),
      TESTID.settingsSection,
    );
    section.dataset.appId = contribution.app.id;
    section.dataset.slot = contribution.slot.id;
    const head = element("header", "settings-section-head");
    head.append(
      element("strong", "", contribution.slot.label),
      element("span", "muted", contribution.app.id),
    );
    const body = element("div", "settings-section-body");
    section.append(head, body);
    settingsBody.append(section);
    const state: Mounted = {
      root: body,
      contribution,
      dispose: () => body.replaceChildren(),
      cancelled: false,
    };
    settingsMounted.push(state);
    trackMount(
      state,
      body,
      contribution,
      "settings",
      closeSettings,
      () => (settingsMounted.includes(state) ? state : undefined),
    );
  }
}

function openSettings(): void {
  closeDrawer();
  manager.classList.add("hidden");
  managerDispose?.();
  managerDispose = null;
  settings.classList.remove("hidden");
  renderSettings();
}

function closeSettings(): void {
  settings.classList.add("hidden");
  disposeSettings();
  settingsBody.replaceChildren();
  settingsSignature = "";
}

function openManager(): void {
  closeDrawer();
  closeSettings();
  manager.classList.remove("hidden");
  managerDispose?.();
  managerDispose = mountManager(managerBody, {
    request: (input, init) => request(input, init),
    onChanged: () => refresh(),
  });
}

function closeManager(): void {
  manager.classList.add("hidden");
  managerDispose?.();
  managerDispose = null;
  managerBody.replaceChildren();
}

function reconcileWindows(): void {
  if (!surface) return;
  const known = new Map(
    [...PORTABLE]
      .flatMap((slot) => ordered(slot))
      .map((contribution) => [contributionKey(contribution), contribution]),
  );
  for (const state of [...floating.values()]) {
    const contribution = known.get(contributionKey(state.contribution));
    if (!contribution) {
      closeFloating(state);
      continue;
    }
    if (state.contribution.slot.sha256 !== contribution.slot.sha256) {
      const kind = state.kind;
      const params = state.params;
      closeFloating(state);
      openWindow(contribution, kind, params);
      continue;
    }
    state.contribution = contribution;
    renderWindowFabs(state);
  }
}

function reconcile(): void {
  const views = ordered(VIEW);
  const fresh = new Map(views.map((contribution) => [contributionKey(contribution), contribution]));
  if (!initialViewOpened) {
    initialViewOpened = true;
    if (tabs.size === 0) {
      const home = defaultView(views.filter((contribution) => implementedByPanel(contribution)));
      if (home) openTab(home);
    }
  }
  for (const key of [...tabs.keys()]) {
    const updated = fresh.get(key);
    if (!updated) {
      closeTab(key);
      continue;
    }
    tabs.get(key)!.contribution = updated;
  }
  if (activeKey && !tabs.has(activeKey)) {
    activeKey = lastTabKey();
  }
  reconcileWindows();
  renderSidebar();
  renderActions();
  renderTabs();
  renderPanes();
  renderFabs();
  renderWarnings();
  if (!settings.classList.contains("hidden")) renderSettings();
  renderStatus();
}

function renderStatus(): void {
  if (!surface) return;
  const apps = surface.apps.length;
  const degraded = surface.degraded.length;
  const text = `${engineVersion ? `engine ${engineVersion} · ` : ""}${apps} app${
    apps === 1 ? "" : "s"
  }${degraded > 0 ? ` · ${degraded} degraded` : ""}`;
  status(degraded > 0 || surface.fatal ? "warn" : "ok", text);
}

async function loadPanelConfig(): Promise<void> {
  try {
    const response = await request("/panel/config.json", {
      headers: { accept: "application/json" },
    });
    if (!response.ok) return;
    const config = (await response.json()) as {
      version?: string;
      slots?: string[];
    };
    if (config.version) panelVersion = config.version;
    if (panelVersionElement) panelVersionElement.textContent = `v${panelVersion}`;
    if (Array.isArray(config.slots) && config.slots.length > 0) {
      implemented = config.slots;
    }
  } catch {
    // The shell still works with the portable vocabulary compiled in.
  }
}

async function loadEngineVersion(): Promise<void> {
  try {
    const response = await request("/api/engine/version", {
      headers: { accept: "application/json" },
    });
    if (!response.ok) return;
    const value = (await response.json()) as { engine?: string };
    engineVersion = value.engine ?? "";
  } catch {
    // Composition is the signal that matters; this is decoration.
  }
}

async function refresh(): Promise<void> {
  try {
    const response = await request("/api/engine/composition", {
      headers: { accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(await errorMessage(response));
    }
    const value = (await response.json()) as CompositionView;
    surface = value.surface;
    retry = 0;
    if (!engineVersion && !versionPending) {
      versionPending = true;
      void loadEngineVersion().then(() => {
        versionPending = false;
        renderStatus();
      });
    }
    reconcile();
    document.body.dataset.shellState = "ready";
  } catch (error) {
    document.body.dataset.shellState = "offline";
    status(
      "offline",
      `engine unreachable — retrying (${error instanceof Error ? error.message : String(error)})`,
    );
    scheduleRetry();
  }
}

async function errorMessage(response: Response): Promise<string> {
  try {
    const value = (await response.json()) as {
      error?: { message?: string };
    };
    return value.error?.message ?? `${response.status} ${response.statusText}`;
  } catch {
    return `${response.status} ${response.statusText}`;
  }
}

function scheduleRetry(): void {
  retry += 1;
  const delay = Math.min(15_000, 500 * 2 ** retry);
  window.clearTimeout(retryTimer);
  retryTimer = window.setTimeout(() => void refresh(), delay);
}

navToggle.addEventListener("click", () => {
  if (drawerOpen) closeDrawer();
  else setDrawer(true);
});
drawerClose.addEventListener("click", closeDrawer);
drawerBackdrop.addEventListener("click", closeDrawer);
viewportSelect.addEventListener("change", () => {
  if (!isViewportMode(viewportSelect.value)) return;
  viewportMode = viewportSelect.value;
  storeViewport(viewportMode);
  applyViewport();
});
developer.querySelector("summary")?.addEventListener("click", () => {
  developerTouched = true;
});
window.addEventListener("resize", () => {
  if (viewportMode === "auto") applyViewport();
});

settingsButton.addEventListener("click", () => {
  if (settings.classList.contains("hidden")) openSettings();
  else closeSettings();
});
settingsClose.addEventListener("click", closeSettings);
settings.addEventListener("pointerdown", (event) => {
  if (event.target === settings) closeSettings();
});
managerButton.addEventListener("click", () => {
  if (manager.classList.contains("hidden")) openManager();
  else closeManager();
});
managerClose.addEventListener("click", closeManager);
manager.addEventListener("pointerdown", (event) => {
  if (event.target === manager) closeManager();
});
modalClose.addEventListener("click", () => closeModal());
modalElement.addEventListener("close", () => closeModal());
modalElement.addEventListener("pointerdown", (event) => {
  if (event.target === modalElement) closeModal();
});
document.addEventListener("pointerdown", (event) => {
  const target = event.target;
  if (!(target instanceof Element) || !target.closest(".fabs")) {
    closeFabMenus();
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  // A modal is in the top layer: the dialog handles Escape and its close
  // event refreshes the view. The shell must not close windows behind it.
  if (modalElement.open) return;
  if (document.querySelector(".fab-menu:not([hidden])")) {
    closeFabMenus();
    return;
  }
  if (!manager.classList.contains("hidden")) {
    closeManager();
    return;
  }
  if (!settings.classList.contains("hidden")) {
    closeSettings();
    return;
  }
  if (drawerOpen) {
    closeDrawer();
    return;
  }
  const top = [...floating.values()]
    .filter((state) => state.mode !== "minimized")
    .sort(
      (left, right) =>
        Number(left.element.style.zIndex) - Number(right.element.style.zIndex),
    )
    .at(-1);
  if (top) closeFloating(top);
});

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) void refresh();
});
window.addEventListener("focus", () => void refresh());

applyViewport();
void loadPanelConfig().then(() => refresh());
window.setInterval(() => {
  if (!document.hidden) void refresh();
}, 10_000);
