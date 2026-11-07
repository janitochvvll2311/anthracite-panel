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
  contributionKey,
  ignored as sdkIgnored,
  mountFrame,
  ordered as orderContributions,
  type App,
  type Contribution,
  type FrameContext,
} from "@anthracite/panel";

import {
  VIEWPORT_KEY,
  isViewportMode,
  resolveViewport,
  type ResolvedViewport,
  type ViewportMode,
} from "./viewport";

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
};

type Mounted = {
  root: HTMLElement;
  contribution: Contribution;
  dispose: () => void;
  /// Set when the host closes or replaces the mount; a late module must not
  /// attach to a detached root.
  cancelled: boolean;
};

type Floating = Mounted & {
  key: string;
  element: HTMLElement;
};

const byId = <T extends HTMLElement>(id: string): T => {
  const element = document.getElementById(id);
  if (!element) throw new Error(`missing #${id}`);
  return element as T;
};

const health = byId<HTMLSpanElement>("health");
const views = byId<HTMLElement>("views");
const actions = byId<HTMLElement>("actions");
const warnings = byId<HTMLElement>("warnings");
const tabsElement = byId<HTMLElement>("tabs");
const panes = byId<HTMLElement>("panes");
const empty = byId<HTMLElement>("empty");
const windowsElement = byId<HTMLElement>("windows");
const settings = byId<HTMLElement>("settings");
const settingsBody = byId<HTMLElement>("settings-body");
const settingsButton = byId<HTMLButtonElement>("settings-button");
const settingsClose = byId<HTMLButtonElement>("settings-close");
const viewportSelect = byId<HTMLSelectElement>("viewport-select");
const viewportEffective = byId<HTMLElement>("viewport-effective");
const navToggle = byId<HTMLButtonElement>("nav-toggle");
const drawerClose = byId<HTMLButtonElement>("drawer-close");
const drawerBackdrop = byId<HTMLElement>("drawer-backdrop");
const sidebar = byId<HTMLElement>("sidebar");

let surface: Surface | null = null;
let implemented: string[] = [...PORTABLE];
let panelVersion = "0.5.0";
let engineVersion = "";
let versionPending = false;
let activeKey: string | null = null;
let retry = 0;
let retryTimer: number | undefined;
let zIndex = 10;
const panelVersionElement = document.getElementById("panel-version");
let settingsSignature = "";

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
let viewportMode: ViewportMode = isViewportMode(requestedViewport)
  ? requestedViewport
  : isViewportMode(storedViewport)
    ? storedViewport
    : "auto";
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

function status(kind: "ok" | "warn" | "offline", text: string): void {
  health.dataset.state = kind;
  health.textContent = text;
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function button(className: string, text: string, title?: string): HTMLButtonElement {
  const node = element("button", className, text);
  node.type = "button";
  if (title) node.title = title;
  return node;
}

function renderSidebar(): void {
  views.replaceChildren();
  if (!surface) {
    views.append(element("div", "placeholder", "Reading the composition…"));
    return;
  }

  const contributions = ordered(VIEW).filter(implementedByPanel);
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
  for (const contribution of ordered(ACTION).filter(implementedByPanel)) {
    const node = button(
      "action-button",
      contribution.slot.label,
      `${contribution.slot.label} — ${contribution.app.id}`,
    );
    const badge = element("span", "action-icon", "⚡");
    node.prepend(badge);
    node.addEventListener("click", () => openWindow(contribution, "action"));
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

    const detach = button("tab-tool", "⇱", "Open in a floating window");
    detach.addEventListener("click", (event) => {
      event.stopPropagation();
      detachTab(tab.key);
    });

    const close = button("tab-tool", "×", "Close");
    close.addEventListener("click", (event) => {
      event.stopPropagation();
      closeTab(tab.key);
    });

    node.append(select, detach, close);
    tabsElement.append(node);
  }
}

function contextFor(
  contribution: Contribution,
  kind: FrameContext["kind"],
): FrameContext {
  return {
    app: {
      id: contribution.app.id,
      version: contribution.app.version,
      risk: contribution.app.risk,
    },
    contribution: { ...contribution.slot, app: contribution.app.id },
    panel: { slots: [...implemented], version: panelVersion },
    kind,
  };
}

function errorCard(error: unknown): HTMLElement {
  const card = element("div", "error-card");
  card.append(
    element("strong", "", "This module did not mount"),
    element("p", "", error instanceof Error ? error.message : String(error)),
  );
  return card;
}

function safeDispose(dispose: () => void): void {
  try {
    dispose();
  } catch (error) {
    console.error("disposing a module failed", error);
  }
}

async function mountInto(
  root: HTMLElement,
  contribution: Contribution,
  kind: FrameContext["kind"],
  close: () => void,
  cancelled: () => boolean,
): Promise<() => void> {
  root.replaceChildren(element("div", "loading", "Loading module…"));
  try {
    const response = await fetch(assetUrl(contribution), {
      headers: { accept: "text/javascript" },
    });
    if (!response.ok) throw new Error(await errorMessage(response));
    const source = await response.text();
    if (cancelled()) return () => root.replaceChildren();
    const dispose = await mountFrame(root, {
      source,
      context: contextFor(contribution, kind),
      close,
      request: (input, init) => fetch(input, init),
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

function ensurePane(contribution: Contribution): void {
  const key = contributionKey(contribution);
  const existing = mounted.get(key);
  if (existing) {
    if (existing.contribution.slot.sha256 !== contribution.slot.sha256) {
      existing.cancelled = true;
      safeDispose(existing.dispose);
      existing.root.parentElement?.remove();
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
  detach.addEventListener("click", () => detachTab(key));
  const close = button("ghost small", "Close", "Close this view");
  close.addEventListener("click", () => closeTab(key));
  tools.append(detach, close);
  head.append(copy, tools);

  const body = element("div", "pane-body");
  pane.append(head, body);
  panes.append(pane);

  const state: Mounted = {
    root: body,
    contribution,
    dispose: () => body.replaceChildren(),
    cancelled: false,
  };
  mounted.set(key, state);
  void mountInto(
    body,
    contribution,
    "view",
    () => closeTab(key),
    () => state.cancelled,
  ).then((dispose) => {
    if (state.cancelled || mounted.get(key) !== state) {
      safeDispose(dispose);
      return;
    }
    state.dispose = dispose;
  });
}

function renderPanes(): void {
  for (const [key, state] of mounted) {
    if (!tabs.has(key)) {
      state.cancelled = true;
      safeDispose(state.dispose);
      state.root.parentElement?.remove();
      mounted.delete(key);
    }
  }
  for (const tab of tabs.values()) ensurePane(tab.contribution);
  for (const [key, state] of mounted) {
    const pane = state.root.parentElement;
    if (!pane) continue;
    pane.classList.toggle("active", key === activeKey);
  }
  empty.classList.toggle("hidden", tabs.size > 0);
}

function openTab(contribution: Contribution, activateTab = true): void {
  const key = contributionKey(contribution);
  const existing = tabs.get(key);
  if (existing) existing.contribution = contribution;
  else tabs.set(key, { key, contribution });
  if (activateTab) activate(key);
  renderTabs();
  renderPanes();
  renderSidebar();
  closeDrawer();
}

function activate(key: string): void {
  activeKey = key;
  renderTabs();
  renderPanes();
  renderSidebar();
}

function closeTab(key: string): void {
  const state = mounted.get(key);
  if (state) {
    state.cancelled = true;
    safeDispose(state.dispose);
    state.root.parentElement?.remove();
    mounted.delete(key);
  }
  tabs.delete(key);
  if (activeKey === key) {
    activeKey = tabs.size > 0 ? [...tabs.keys()][tabs.size - 1] : null;
  }
  renderTabs();
  renderPanes();
  renderSidebar();
}

function detachTab(key: string): void {
  const tab = tabs.get(key);
  if (!tab) return;
  openWindow(tab.contribution, "view");
  closeTab(key);
}

function focusWindow(state: Floating): void {
  state.element.style.zIndex = String(++zIndex);
}

function openWindow(
  contribution: Contribution,
  kind: FrameContext["kind"],
): void {
  const key = `window:${kind}:${contributionKey(contribution)}`;
  const existing = floating.get(key);
  if (existing) {
    if (existing.contribution.slot.sha256 === contribution.slot.sha256) {
      focusWindow(existing);
      return;
    }
    existing.cancelled = true;
    safeDispose(existing.dispose);
    existing.element.remove();
    floating.delete(key);
  }

  const offset = 74 + floating.size * 28;
  const windowElement = element("section", "window");
  windowElement.style.left = `${Math.min(offset, window.innerWidth - 480)}px`;
  windowElement.style.top = `${Math.min(offset, window.innerHeight - 320)}px`;

  const header = element("header", "window-head");
  const copy = element("div", "window-copy");
  copy.append(
    element("strong", "", contribution.slot.label),
    element("span", "window-app", contribution.app.id),
  );
  const close = button("ghost small", "×", "Close");
  header.append(copy, close);

  const body = element("div", "window-body");
  windowElement.append(header, body);
  windowsElement.append(windowElement);

  const state: Floating = {
    key,
    element: windowElement,
    root: body,
    contribution,
    dispose: () => body.replaceChildren(),
    cancelled: false,
  };
  floating.set(key, state);
  focusWindow(state);

  const closeWindow = (): void => {
    state.cancelled = true;
    safeDispose(state.dispose);
    windowElement.remove();
    floating.delete(key);
  };
  close.addEventListener("click", closeWindow);
  windowElement.addEventListener("pointerdown", () => focusWindow(state));
  makeDraggable(windowElement, header);
  void mountInto(body, contribution, kind, closeWindow, () => state.cancelled).then(
    (dispose) => {
      if (state.cancelled || floating.get(key) !== state) {
        safeDispose(dispose);
        return;
      }
      state.dispose = dispose;
    },
  );
}

function makeDraggable(elementNode: HTMLElement, handle: HTMLElement): void {
  let originX = 0;
  let originY = 0;
  let pointerId = -1;

  handle.addEventListener("pointerdown", (event) => {
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
    state.cancelled = true;
    safeDispose(state.dispose);
  }
  settingsMounted = [];
}

function renderSettings(): void {
  const contributions = ordered(SETTINGS).filter(implementedByPanel);
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
    void mountInto(
      body,
      contribution,
      "settings",
      closeSettings,
      () => state.cancelled,
    ).then((dispose) => {
      if (state.cancelled) {
        safeDispose(dispose);
        return;
      }
      state.dispose = dispose;
    });
  }
}

function openSettings(): void {
  settings.classList.remove("hidden");
  renderSettings();
}

function closeSettings(): void {
  settings.classList.add("hidden");
  disposeSettings();
  settingsBody.replaceChildren();
  settingsSignature = "";
}

function reconcileWindows(): void {
  if (!surface) return;
  const known = new Map(
    [...PORTABLE]
      .flatMap((slot) => ordered(slot))
      .map((contribution) => [contributionKey(contribution), contribution]),
  );
  for (const [key, state] of floating) {
    const contribution = known.get(contributionKey(state.contribution));
    if (!contribution) {
      state.cancelled = true;
      safeDispose(state.dispose);
      state.element.remove();
      floating.delete(key);
      continue;
    }
    if (state.contribution.slot.sha256 !== contribution.slot.sha256) {
      state.cancelled = true;
      safeDispose(state.dispose);
      state.element.remove();
      floating.delete(key);
      openWindow(contribution, key.includes(":action:") ? "action" : "view");
    }
  }
}

function reconcile(): void {
  const views = ordered(VIEW);
  const fresh = new Map(views.map((contribution) => [contributionKey(contribution), contribution]));
  for (const key of [...tabs.keys()]) {
    const updated = fresh.get(key);
    if (!updated) {
      closeTab(key);
      continue;
    }
    tabs.get(key)!.contribution = updated;
  }
  if (activeKey && !tabs.has(activeKey)) {
    activeKey = tabs.size > 0 ? [...tabs.keys()][tabs.size - 1] : null;
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
    const response = await fetch("/panel/config.json", {
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
    const response = await fetch("/api/engine/version", {
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
    const response = await fetch("/api/engine/composition", {
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
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!settings.classList.contains("hidden")) {
    closeSettings();
    return;
  }
  if (drawerOpen) {
    closeDrawer();
    return;
  }
  const top = [...floating.values()].sort(
    (left, right) =>
      Number(left.element.style.zIndex) - Number(right.element.style.zIndex),
  ).at(-1);
  if (top) {
    top.dispose();
    top.element.remove();
    floating.delete(top.key);
  }
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
