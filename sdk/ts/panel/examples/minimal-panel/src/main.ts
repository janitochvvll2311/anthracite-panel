// The second panel. It knows only the SDK: it reads the composition, filters
// `panel.view`, orders and mounts the contributions with @anthracite/panel, and
// implements a tab that can be detached. It never imports the official panel or
// any app.

import {
  VIEW,
  assetUrl,
  contributionKey,
  mountFrame,
  type App,
  type Contribution,
  type FrameContext,
} from "../../../src/index";

import { ignoredViews, visibleViews } from "./contract";

type Surface = {
  apps: App[];
  degraded: { app: string; reason: string }[];
};

type CompositionView = {
  surface: Surface;
};

type Tab = {
  key: string;
  contribution: Contribution;
  element: HTMLElement;
  body: HTMLElement;
  dispose: () => void;
  cancelled: boolean;
  detached: boolean;
  generation: number;
};

const byId = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

const views = byId("views");
const ignored = byId("ignored");
const tabsElement = byId("tabs");
const panes = byId("panes");
const empty = byId("empty");
const status = byId("status");

const tabs = new Map<string, Tab>();
let apps: App[] = [];
let degraded: Surface["degraded"] = [];
let active: string | null = null;
let retry = 0;

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

function setStatus(kind: "ok" | "bad", text: string): void {
  status.dataset.state = kind;
  status.textContent = text;
}

function viewsFor(): Contribution[] {
  return visibleViews(apps);
}

function safeDispose(dispose: () => void): void {
  try {
    dispose();
  } catch (error) {
    console.error("disposing a view failed", error);
  }
}

function contextFor(contribution: Contribution): FrameContext {
  return {
    app: {
      id: contribution.app.id,
      version: contribution.app.version,
      risk: contribution.app.risk,
    },
    contribution: { ...contribution.slot, app: contribution.app.id },
    panel: { slots: [VIEW], version: "0.5.0-second" },
    kind: "view",
  };
}

function errorCard(error: unknown): HTMLElement {
  const card = element("div", "error");
  card.append(
    element("strong", "", "This view did not mount"),
    element("span", "", error instanceof Error ? error.message : String(error)),
  );
  return card;
}

async function mount(tab: Tab): Promise<void> {
  const generation = ++tab.generation;
  tab.body.replaceChildren(element("div", "loading", "Loading view…"));
  try {
    const response = await fetch(assetUrl(tab.contribution), {
      headers: { accept: "text/javascript" },
    });
    if (!response.ok) throw new Error(`the module answered ${response.status}`);
    const source = await response.text();
    if (tab.cancelled || tab.generation !== generation) return;
    const dispose = await mountFrame(tab.body, {
      source,
      context: contextFor(tab.contribution),
      close: () => close(tab.key),
      request: (input, init) => fetch(input, init),
    });
    if (
      tab.cancelled ||
      tab.generation !== generation ||
      tabs.get(tab.key) !== tab
    ) {
      safeDispose(dispose);
      return;
    }
    tab.dispose = dispose;
  } catch (error) {
    if (!tab.cancelled && tab.generation === generation) {
      tab.body.replaceChildren(errorCard(error));
    }
  }
}

function close(key: string): void {
  const tab = tabs.get(key);
  if (!tab) return;
  tab.cancelled = true;
  tab.generation += 1;
  safeDispose(tab.dispose);
  tab.element.remove();
  tabs.delete(key);
  if (active === key) {
    active = tabs.size > 0 ? [...tabs.keys()][tabs.size - 1] : null;
  }
  render();
}

function detach(key: string): void {
  const tab = tabs.get(key);
  if (!tab) return;
  // Keep the iframe in place: reparenting it would reload the module and lose
  // the app state. The pane becomes fixed and floats above the workspace.
  tab.detached = true;
  active = key;
  render();
}

function dock(key: string): void {
  const tab = tabs.get(key);
  if (!tab) return;
  tab.detached = false;
  active = key;
  render();
}

function open(contribution: Contribution): void {
  const key = contributionKey(contribution);
  const existing = tabs.get(key);
  if (existing) {
    active = key;
    render();
    return;
  }

  const pane = element("section", "pane");
  pane.dataset.key = key;
  const head = element("header", "pane-head");
  const label = element("strong", "", contribution.slot.label);
  const owner = element("span", "owner", contribution.app.id);
  const tools = element("div", "tools");
  const detachButton = button("ghost small", "Detach", "Move to a floating pane");
  detachButton.addEventListener("click", () => detach(key));
  const dockButton = button("ghost small", "Dock", "Return to the tab strip");
  dockButton.addEventListener("click", () => dock(key));
  const closeButton = button("ghost small", "×", "Close");
  closeButton.addEventListener("click", () => close(key));
  tools.append(detachButton, dockButton, closeButton);
  head.append(label, owner, tools);
  const body = element("div", "pane-body");
  pane.append(head, body);
  panes.append(pane);

  const tab: Tab = {
    key,
    contribution,
    element: pane,
    body,
    dispose: () => body.replaceChildren(),
    cancelled: false,
    detached: false,
    generation: 0,
  };
  tabs.set(key, tab);
  active = key;
  void mount(tab);
  render();
}

function renderSidebar(): void {
  views.replaceChildren();
  const contributions = viewsFor();
  if (contributions.length === 0) {
    views.append(element("div", "placeholder", "No app contributes a view."));
  }
  for (const contribution of contributions) {
    const key = contributionKey(contribution);
    const node = button("view", "", `${contribution.slot.label} — ${contribution.app.id}`);
    if (key === active) node.classList.add("active");
    node.append(
      element("span", "view-label", contribution.slot.label),
      element("span", "view-app", contribution.app.id),
    );
    node.addEventListener("click", () => open(contribution));
    views.append(node);
  }

  const unhandled = ignoredViews(apps);
  ignored.replaceChildren();
  if (unhandled.length > 0) {
    ignored.append(
      element(
        "div",
        "note",
        `${unhandled.length} contribution(s) outside panel.view are ignored by this panel.`,
      ),
    );
  }
  if (degraded.length > 0) {
    ignored.append(
      element(
        "div",
        "note",
        `${degraded.length} app(s) degraded: ${degraded
          .map((app) => app.app)
          .join(", ")}`,
      ),
    );
  }
}

function renderTabs(): void {
  tabsElement.replaceChildren();
  for (const tab of tabs.values()) {
    if (tab.detached) continue;
    const node = element("div", "tab");
    if (tab.key === active) node.classList.add("active");
    const select = button("tab-label", tab.contribution.slot.label);
    select.title = tab.contribution.app.id;
    select.addEventListener("click", () => {
      active = tab.key;
      render();
    });
    node.append(select);
    tabsElement.append(node);
  }
}

function renderPanes(): void {
  for (const tab of tabs.values()) {
    tab.element.classList.toggle("floating", tab.detached);
    tab.element.classList.toggle("active", tab.detached || tab.key === active);
  }
  empty.classList.toggle("hidden", tabs.size > 0);
}

function render(): void {
  renderSidebar();
  renderTabs();
  renderPanes();
}

function reconcile(): void {
  const fresh = new Map(
    viewsFor().map((contribution) => [contributionKey(contribution), contribution]),
  );
  for (const tab of [...tabs.values()]) {
    const updated = fresh.get(tab.key);
    if (!updated) {
      close(tab.key);
      continue;
    }
    if (updated.slot.sha256 !== tab.contribution.slot.sha256) {
      tab.cancelled = true;
      tab.generation += 1;
      safeDispose(tab.dispose);
      tab.cancelled = false;
      tab.contribution = updated;
      void mount(tab);
    } else {
      tab.contribution = updated;
    }
  }
  render();
}

async function refresh(): Promise<void> {
  try {
    const response = await fetch("/api/engine/composition", {
      headers: { accept: "application/json" },
    });
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
    const value = (await response.json()) as CompositionView;
    apps = value.surface.apps;
    degraded = value.surface.degraded;
    retry = 0;
    setStatus(
      degraded.length > 0 ? "bad" : "ok",
      `${apps.length} app${apps.length === 1 ? "" : "s"} · ${viewsFor().length} view${
        viewsFor().length === 1 ? "" : "s"
      }`,
    );
    reconcile();
  } catch (error) {
    setStatus("bad", `disconnected — retrying (${error instanceof Error ? error.message : error})`);
    retry += 1;
    window.setTimeout(() => void refresh(), Math.min(15_000, 500 * 2 ** retry));
  }
}

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && active) close(active);
});

void refresh();
window.setInterval(() => {
  if (!document.hidden) void refresh();
}, 10_000);
