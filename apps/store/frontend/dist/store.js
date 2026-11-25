// src/api.ts
function route(context, path) {
  return `/api/apps/${encodeURIComponent(context.app.id)}${path}`;
}
async function call(context, path, init = {}) {
  const { headers, ...rest } = init;
  const response = await context.fetch(route(context, path), {
    ...rest,
    headers: { accept: "application/json", ...headers ?? {} }
  });
  const body = await response.text();
  let value = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message = value && typeof value === "object" && value.error ? `${value.error.code}: ${value.error.message}` : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}

// src/state.ts
function channelKind(channel) {
  if (channel === "stable")
    return "ok";
  if (channel === "beta")
    return "warn";
  if (channel === "nightly")
    return "off";
  return "off";
}
function formatBytes(bytes) {
  if (!bytes)
    return "0 B";
  const units = ["B", "KiB", "MiB", "GiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}
function initials(name) {
  const parts = name.split(/[\s._-]+/).filter(Boolean).slice(0, 2);
  if (parts.length === 0)
    return "?";
  return parts.map((part) => part[0]?.toUpperCase() ?? "").join("");
}
function shortDigest(digest) {
  return digest.length > 12 ? `${digest.slice(0, 12)}…` : digest;
}

// src/styles.ts
var styles = `
  .st { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .st-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .st-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .st-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .st button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .st button:hover { border-color: var(--accent); }
  .st button.primary { border-color: var(--accent); background: var(--panel-3); }
  .st button:disabled { opacity: .55; cursor: default; }
  .st input[type="search"] {
    padding: 5px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 12px; min-width: 180px; flex: 1 1 220px;
  }
  .st-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .st-status.bad { color: var(--danger); }
  .st-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; }
  .st-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0; cursor: pointer;
  }
  .st-card:hover { border-color: var(--line); }
  .st-card.selected { border-color: var(--accent); }
  .st-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .st-initials {
    width: 30px; height: 30px; border-radius: 8px; display: grid; place-items: center;
    background: var(--panel-3); color: var(--accent); font-size: 12px; font-weight: 600;
  }
  .st-card-head strong { font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .st-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .st-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .st-badge.ok { border-color: var(--good); color: var(--good); }
  .st-badge.warn { border-color: var(--warn); color: var(--warn); }
  .st-badge.bad { border-color: var(--danger); color: var(--danger); }
  .st-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px; color: var(--muted); font-size: 12px;
  }
  .st-note { font-size: 12px; color: var(--muted); }
  .st-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .st-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .st-detail-head strong { font-size: 13px; color: var(--text); }
  .st-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .st-versions { display: flex; flex-direction: column; gap: 4px; }
  .st-version {
    display: grid; grid-template-columns: 110px 80px 90px 1fr 90px;
    gap: 8px; align-items: center; padding: 5px 4px; border-bottom: 1px solid var(--line-soft); font-size: 12px;
  }
  .st-version:last-child { border-bottom: 0; }
  .st-version .yanked { color: var(--danger); }
  .st-sources { display: flex; flex-direction: column; gap: 8px; }
  .st-source {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    border: 1px solid var(--line-soft); border-radius: 9px; padding: 8px 10px; background: var(--bg-soft);
  }

  html[data-viewport="mobile"] .st-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .st-version { grid-template-columns: minmax(0, 1fr) 80px; }
  html[data-viewport="mobile"] .st-version > :nth-child(3),
  html[data-viewport="mobile"] .st-version > :nth-child(4) { display: none; }

  @container (max-width: 640px) {
    .st-grid { grid-template-columns: minmax(0, 1fr); }
  }
`;

// src/main.ts
function escape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function date(seconds) {
  if (!seconds)
    return "—";
  return new Date(seconds * 1000).toLocaleDateString();
}
function mount(root, context) {
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
  const status = box.querySelector("[data-status]");
  const counts = box.querySelector("[data-counts]");
  const results = box.querySelector("[data-results]");
  const detail = box.querySelector("[data-detail]");
  const search = box.querySelector("[data-search]");
  let sources = [];
  let apps = [];
  let selected = null;
  let current = null;
  let query = "";
  let visible = true;
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "st-status bad" : "st-status";
  };
  function renderCounts() {
    const trusted = sources.filter((source) => source.trusted).length;
    counts.textContent = `${apps.length} apps · ${trusted}/${sources.length} sources trusted`;
  }
  function renderResults() {
    results.replaceChildren();
    if (apps.length === 0) {
      const empty = document.createElement("div");
      empty.className = "st-empty";
      empty.textContent = sources.length === 0 ? "No source configured. Add one to {data}/sources.json and refresh." : "No app matches this search. Refresh the catalogues or try another term.";
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
  function versionRow(version) {
    return `<div class="st-version" data-testid="store-version" data-version="${escape(version.version)}">
      <span class="st-mono">${escape(version.version)}</span>
      <span class="st-badge ${channelKind(version.channel)}">${escape(version.channel)}</span>
      <span>${escape(date(version["published-at"]))}</span>
      <span class="st-mono" title="${escape(version.sha256)}">${escape(shortDigest(version.sha256))} · ${escape(formatBytes(version.size))}</span>
      <span class="${version.yanked ? "yanked" : "st-mono"}">${version.yanked ? "yanked" : "ok"}</span>
    </div>`;
  }
  function renderDetail() {
    if (!current) {
      detail.hidden = true;
      detail.replaceChildren();
      return;
    }
    detail.hidden = false;
    detail.replaceChildren();
    const head = document.createElement("div");
    head.className = "st-detail-head";
    head.innerHTML = `<strong>${escape(current.name)}</strong><span class="st-mono">${escape(current.id)}</span><span class="st-badge">${escape(current.publisher)}</span>`;
    const facts = document.createElement("div");
    facts.className = "st-actions";
    const fact = (label, value) => {
      const node = document.createElement("span");
      node.className = "st-field";
      node.innerHTML = `<span class="st-field-name">${escape(label)}</span><span>${escape(value)}</span>`;
      return node;
    };
    facts.append(fact("License", current.license || "—"), fact("Homepage", current.homepage ?? "—"), fact("Categories", current.categories.join(", ") || "—"));
    const description = document.createElement("p");
    description.className = "st-note";
    description.textContent = current.description || current.summary;
    const versions = document.createElement("div");
    versions.className = "st-versions";
    versions.innerHTML = `<div class="st-version st-field-name"><span>Version</span><span>Channel</span><span>Published</span><span>Digest</span><span>State</span></div>${current.versions.map(versionRow).join("")}`;
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
  function render() {
    renderCounts();
    renderResults();
    renderDetail();
  }
  async function loadSources() {
    sources = await call(context, "/sources") ?? [];
  }
  async function load() {
    try {
      await loadSources();
      apps = await call(context, `/search?query=${encodeURIComponent(query)}&limit=100`) ?? [];
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
  async function select(id) {
    selected = id;
    render();
    try {
      current = await call(context, `/app?id=${encodeURIComponent(id)}`);
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function refresh() {
    try {
      setStatus("Refreshing catalogues…");
      let failures = 0;
      for (const source of sources) {
        try {
          await call(context, "/sources/refresh", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ source: source.id })
          });
        } catch {
          failures += 1;
        }
      }
      await load();
      setStatus(failures ? `${failures} source(s) failed; the cache was kept.` : "Catalogues refreshed.", failures > 0);
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  let timer = null;
  search.addEventListener("input", () => {
    query = search.value;
    if (timer !== null)
      window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      timer = null;
      load();
    }, 250);
  });
  box.querySelector("[data-refresh]")?.addEventListener("click", () => void refresh());
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh")
      refresh();
  });
  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    if (next)
      load();
  });
  load();
  const poll = window.setInterval(() => {
    if (visible && !disposed)
      loadSources().then(renderCounts);
  }, 30000);
  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    if (timer !== null)
      window.clearTimeout(timer);
    window.clearInterval(poll);
    root.replaceChildren();
  };
}
export {
  mount
};
