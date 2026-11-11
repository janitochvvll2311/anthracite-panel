//! The official panel shell.
//!
//! It knows nothing about apps: it reads the composition surface, mounts the
//! modules an app contributes to the three portable slots, and keeps the layout
//! in the panel. The engine serves the modules; the panel never touches the
//! registry, the grants or the state tree.

import {
  ACTION,
  PORTABLE,
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
  type App,
  type Contribution,
  type FrameContext,
  type FrameHandle,
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
  /// Sends a transient open request to a live frame without remounting it.
  update?: (params: unknown) => void;
  /// Asks the live frame for permission before closing (unsaved changes).
  requestClose?: () => Promise<boolean>;
  /// Reads the current view state before moving it (float/dock).
  snapshot?: () => Promise<unknown>;
  /// Pauses app polling while the pane is not the active tab.
  setVisible?: (visible: boolean) => void;
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
};

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
let panelVersion = "0.5.0";
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

function icon(value: string | undefined, fallback: string): string {
  const trimmed = value?.trim();
  if (!trimmed) return fallback;
  // The manifest carries names, not markup. One character keeps the sidebar
  // calm and avoids injecting arbitrary content into the DOM.
  return [...trimmed][0] ?? fallback;
}

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
    views.append(element("div", "placeholder", "Reading the composition…"));
    return;
  }

  const contributions = ordered(VIEW).filter(
    (contribution) => implementedByPanel(contribution) && !contribution.slot.hidden,
  );
  if (contributions.length === 0) {
    views.append(element("div", "placeholder", "No app contributes a view."));
    return;
  }

  const label = element("div", "sidebar-title", "Views");
  views.append(label);
  for (const contribution of contributions) {
    const key = contributionKey(contribution);
    const node = button(
      "view-button",
      "",
      `${contribution.slot.label} — ${contribution.app.id}`,
    );
    node.dataset.key = key;
    if (key === activeKey) node.classList.add("active");

    const badge = element(
      "span",
      "view-icon",
      icon(contribution.slot.icon, [...contribution.slot.label][0] ?? "•"),
    );
    const copy = element("span", "view-copy");
    copy.append(
      element("span", "view-label", contribution.slot.label),
      element("span", "view-app", contribution.app.id),
    );
    node.append(badge, copy);
    node.addEventListener("click", () => openTab(contribution));
    views.append(node);
  }
}

function renderActions(): void {
  actions.replaceChildren();
  if (!surface) return;
  for (const contribution of ordered(ACTION).filter(
    (candidate) => implementedByPanel(candidate) && !candidate.slot.hidden,
  )) {
    const node = button(
      "action-button",
      contribution.slot.label,
      `${contribution.slot.label} — ${contribution.app.id}`,
    );
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
    const node = element(
      "div",
      "warning fatal",
      `The composition cannot boot: ${surface.fatal.message ?? "unknown error"}`,
    );
    warnings.append(node);
  }
  for (const degraded of surface.degraded) {
    warnings.append(
      element("div", "warning", `${degraded.app}: ${degraded.reason}`),
    );
  }
  const ignored = sdkIgnored(surface.apps, implemented);
  if (ignored.length > 0) {
    warnings.append(
      element(
        "div",
        "warning muted-warning",
        `${ignored.length} contribution(s) target slots this panel does not implement.`,
      ),
    );
  }
}

function renderTabs(): void {
  tabsElement.replaceChildren();
  for (const tab of tabs.values()) {
    const node = element("div", "tab");
    node.dataset.key = tab.key;
    node.setAttribute("role", "tab");
    if (tab.key === activeKey) {
      node.classList.add("active");
      node.setAttribute("aria-selected", "true");
    }

    const select = button("tab-label", tab.contribution.slot.label);
    select.title = tab.contribution.app.id;
    select.addEventListener("click", () => activate(tab.key));

    const detach = button("tab-tool tab-float", "⇱", "Open in a floating window");
    detach.addEventListener("click", (event) => {
      event.stopPropagation();
      void detachTab(tab.key);
    });

    const close = button("tab-tool", "×", "Close");
    close.addEventListener("click", (event) => {
      event.stopPropagation();
      void requestTabClose(tab.key);
    });

    node.append(select, detach, close);
    tabsElement.append(node);
    if (tab.key === activeKey && activeKey !== scrolledActiveKey) {
      scrolledActiveKey = activeKey;
      node.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }
  if (activeKey === null) scrolledActiveKey = null;
  tabsElement.hidden = tabs.size === 0;
  workspace.classList.toggle("no-tabs", tabs.size === 0);
}

async function mountInto(
  root: HTMLElement,
  contribution: Contribution,
  kind: FrameContext["kind"],
  close: () => void,
  cancelled: () => boolean,
  params?: unknown,
  openView?: (request: OpenViewRequest) => boolean | void,
  onParams?: (params: unknown) => void,
  onReady?: (frame: FrameHandle) => void,
): Promise<() => void> {
  root.replaceChildren(element("div", "loading", "Loading module…"));
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
    onParams,
    (frame) => {
      state.update = frame.update;
      state.requestClose = frame.requestClose;
      state.snapshot = frame.snapshot;
      state.setVisible = frame.setVisible;
      const pane = state.root.closest(".pane");
      frame.setVisible(!pane || pane.classList.contains("active"));
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

  const pane = element("section", "pane");
  pane.dataset.key = key;

  const head = element("header", "pane-head");
  const copy = element("div", "pane-copy");
  copy.append(
    element("strong", "", contribution.slot.label),
    element("span", "pane-app", contribution.app.id),
  );
  const tools = element("div", "pane-tools");
  const detach = button("ghost small", "Float", "Open in a floating window");
  detach.addEventListener("click", () => {
    void detachTab(key);
  });
  const close = button("ghost small", "Close", "Close this view");
  close.addEventListener("click", () => {
    void requestTabClose(key);
  });
  tools.append(detach, close);
  head.append(copy, tools);

  const body = element("div", "pane-body");
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
    const pane = state.root.parentElement;
    if (!pane) continue;
    pane.classList.toggle("active", key === activeKey);
    state.setVisible?.(key === activeKey);
  }
  empty.classList.toggle("hidden", tabs.size > 0);
}

function openTab(
  contribution: Contribution,
  activateTab = true,
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

function activate(key: string): void {
  activeKey = key;
  renderWorkspace();
}

function closeTab(key: string): void {
  const state = mounted.get(key);
  if (state) {
    retire(state);
    mounted.delete(key);
  }
  tabs.delete(key);
  if (activeKey === key) {
    activeKey = lastTabKey();
  }
  renderWorkspace();
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
  setMaximizeIcon(state.maximizeButton, false);
}

function minimizeWindow(state: Floating): void {
  if (state.mode === "minimized") return;
  state.restoreMode = minimizedFrom(state.mode);
  state.mode = "minimized";
  state.element.classList.add("minimized");

  const item = element("div", "stack-item");
  const open = button("stack-open", "", "");
  open.title = `Restore ${state.contribution.slot.label}`;
  open.append(
    element("span", "stack-label", state.contribution.slot.label),
    element("span", "stack-app", state.contribution.app.id),
  );
  open.addEventListener("click", () => restoreStack(state));
  const close = button("stack-close", "×", "Close");
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
  const windowElement = element("section", "window");
  windowElement.style.width = `${width}px`;
  windowElement.style.height = `${height}px`;
  windowElement.style.left = `${Math.min(offset, window.innerWidth - width - 12)}px`;
  windowElement.style.top = `${Math.min(offset, window.innerHeight - height - 12)}px`;

  const header = element("header", "window-head");
  const copy = element("div", "window-copy");
  copy.append(
    element("strong", "", contribution.slot.label),
    element("span", "window-app", contribution.app.id),
  );
  const tools = element("div", "window-tools");
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

  const body = element("div", "window-body");
  windowElement.append(header, body);
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
  };
  floating.set(key, state);
  focusWindow(state);

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
      element("p", "placeholder", "No app contributes settings."),
    );
    return;
  }
  for (const contribution of contributions) {
    const section = element("section", "settings-section");
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
    }
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
  } catch (error) {
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
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
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
