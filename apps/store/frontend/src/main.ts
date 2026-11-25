import { call } from "./api";
import { age, channelKind, formatBytes, initials, shortDigest } from "./state";
import { styles } from "./styles";
import type {
  AppDetail,
  FrameContext,
  Source,
  Summary,
  Version,
} from "./types";

function escape(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function date(seconds: number): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleDateString();
}

export function mount(root: HTMLElement, context: FrameContext) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "st";
  box.dataset.testid = "store-view";
  box.innerHTML = `
    <div class="st-bar">
      <span class="st-title">Store</span>
      <span class="st-badge" data-counts></span>
      <span class="grow"></span>
      <input type="search" data-search placeholder="Search apps, publishers, categories" data-testid="store-search" />
      <button type="button" data-refresh data-testid="store-refresh">Refresh</button>
    </div>
    <p class="st-status" role="status" data-status data-testid="store-status"></p>
    <div class="st-grid" data-results data-testid="store-results"></div>
    <section class="st-detail" data-detail data-testid="store-detail" hidden></section>
    <p class="st-note">
      Browsing only. Installing and approving a bundle happens in Manager →
      Store, with the operator session and the engine's computed risk.
    </p>
  `;
  root.append(box);

  const status = box.querySelector("[data-status]") as HTMLElement;
  const counts = box.querySelector("[data-counts]") as HTMLElement;
  const results = box.querySelector("[data-results]") as HTMLElement;
  const detail = box.querySelector("[data-detail]") as HTMLElement;
  const search = box.querySelector("[data-search]") as HTMLInputElement;
  let sources: Source[] = [];
  let apps: Summary[] = [];
  let selected: string | null = null;
  let current: AppDetail | null = null;
  let query = "";
  let visible = true;
  let disposed = false;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "st-status bad" : "st-status";
  };

  function renderCounts(): void {
    const trusted = sources.filter((source) => source.trusted).length;
    counts.textContent = `${apps.length} apps · ${trusted}/${sources.length} sources trusted`;
  }

  function renderResults(): void {
    results.replaceChildren();
    if (apps.length === 0) {
      const empty = document.createElement("div");
      empty.className = "st-empty";
      empty.textContent =
        sources.length === 0
          ? "No source configured. Add one to {data}/sources.json and refresh."
          : "No app matches this search. Refresh the catalogues or try another term.";
      results.append(empty);
      return;
    }
    for (const app of apps) {
      const card = document.createElement("div");
      card.className = `st-card${app.id === selected ? " selected" : ""}`;
      card.dataset.testid = "store-result";
      card.dataset.appId = app.id;
      card.innerHTML = `
        <div class="st-card-head">
          <span class="st-initials">${escape(initials(app.name))}</span>
          <strong title="${escape(app.name)}">${escape(app.name)}</strong>
          <span class="grow"></span>
          <span class="st-badge">${escape(app.latest)}</span>
        </div>
        <div class="st-field"><span class="st-field-name">Publisher</span><span>${escape(app.publisher)}</span></div>
        <div class="st-field"><span class="st-field-name">Summary</span><span>${escape(app.summary)}</span></div>
        <div class="st-field"><span class="st-field-name">Categories</span><span>${escape(app.categories.join(", ") || "—")}</span></div>
        <div class="st-field"><span class="st-field-name">Id</span><span class="st-mono">${escape(app.id)}</span></div>
      `;
      card.addEventListener("click", () => select(app.id));
      results.append(card);
    }
  }

  function versionRow(version: Version): string {
    return `<div class="st-version" data-testid="store-version" data-version="${escape(
      version.version,
    )}">
      <span class="st-mono">${escape(version.version)}</span>
      <span class="st-badge ${channelKind(version.channel)}">${escape(version.channel)}</span>
      <span>${escape(date(version["published-at"]))}</span>
      <span class="st-mono" title="${escape(version.sha256)}">${escape(
        shortDigest(version.sha256),
      )} · ${escape(formatBytes(version.size))}</span>
      <span class="${version.yanked ? "yanked" : "st-mono"}">${
        version.yanked ? "yanked" : "ok"
      }</span>
    </div>`;
  }

  function renderDetail(): void {
    if (!current) {
      detail.hidden = true;
      detail.replaceChildren();
      return;
    }
    detail.hidden = false;
    detail.replaceChildren();
    const head = document.createElement("div");
    head.className = "st-detail-head";
    head.innerHTML = `<strong>${escape(current.name)}</strong><span class="st-mono">${escape(
      current.id,
    )}</span><span class="st-badge">${escape(current.publisher)}</span>`;
    const facts = document.createElement("div");
    facts.className = "st-actions";
    const fact = (label: string, value: string): HTMLElement => {
      const node = document.createElement("span");
      node.className = "st-field";
      node.innerHTML = `<span class="st-field-name">${escape(
        label,
      )}</span><span>${escape(value)}</span>`;
      return node;
    };
    facts.append(
      fact("License", current.license || "—"),
      fact("Homepage", current.homepage ?? "—"),
      fact("Categories", current.categories.join(", ") || "—"),
    );
    const description = document.createElement("p");
    description.className = "st-note";
    description.textContent = current.description || current.summary;
    const versions = document.createElement("div");
    versions.className = "st-versions";
    versions.innerHTML = `<div class="st-version st-field-name"><span>Version</span><span>Channel</span><span>Published</span><span>Digest</span><span>State</span></div>${current.versions
      .map(versionRow)
      .join("")}`;
    const close = document.createElement("div");
    close.className = "st-actions";
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "Close";
    button.addEventListener("click", () => {
      selected = null;
      current = null;
      render();
    });
    close.append(button);
    detail.append(head, facts, description, versions, close);
  }

  function render(): void {
    renderCounts();
    renderResults();
    renderDetail();
  }

  async function loadSources(): Promise<void> {
    sources = (await call(context, "/sources")) ?? [];
  }

  async function load(): Promise<void> {
    try {
      await loadSources();
      apps =
        (await call(
          context,
          `/search?query=${encodeURIComponent(query)}&limit=100`,
        )) ?? [];
      if (selected && !apps.some((app) => app.id === selected)) {
        selected = null;
        current = null;
      }
      setStatus("");
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function select(id: string): Promise<void> {
    selected = id;
    render();
    try {
      current = (await call(
        context,
        `/app?id=${encodeURIComponent(id)}`,
      )) as AppDetail;
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function refresh(): Promise<void> {
    try {
      setStatus("Refreshing catalogues…");
      // A click can land before the first load resolves; sources is still empty.
      if (sources.length === 0) await loadSources();
      let failures = 0;
      for (const source of sources) {
        try {
          await call(context, "/sources/refresh", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ source: source.id }),
          });
        } catch {
          failures += 1;
        }
      }
      await load();
      setStatus(
        failures ? `${failures} source(s) failed; the cache was kept.` : "Catalogues refreshed.",
        failures > 0,
      );
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  let timer: number | null = null;
  search.addEventListener("input", () => {
    query = search.value;
    if (timer !== null) window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      timer = null;
      void load();
    }, 250);
  });
  box.querySelector("[data-refresh]")?.addEventListener("click", () => void refresh());

  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") void refresh();
  });
  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    if (next) void load();
  });
  void load();

  const poll = window.setInterval(() => {
    if (visible && !disposed) void loadSources().then(renderCounts);
  }, 30_000);

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    if (timer !== null) window.clearTimeout(timer);
    window.clearInterval(poll);
    root.replaceChildren();
  };
}
