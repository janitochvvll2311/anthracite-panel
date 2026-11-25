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
function age(seconds, now = Date.now() / 1000) {
  if (!seconds)
    return "never refreshed";
  const delta = Math.max(0, Math.floor(now - seconds));
  if (delta < 60)
    return "just now";
  if (delta < 3600)
    return `${Math.floor(delta / 60)} min ago`;
  if (delta < 86400)
    return `${Math.floor(delta / 3600)} h ago`;
  return `${Math.floor(delta / 86400)} d ago`;
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

// src/modal.ts
var extra = `
  .stm { display: flex; flex-direction: column; gap: 10px; }
  .stm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .stm-status { margin: 0; font-size: 12px; color: var(--muted); }
  .stm-status.bad { color: var(--danger); }
`;
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);
  const box = document.createElement("div");
  box.className = "stm";
  box.dataset.testid = "store-sources-modal";
  box.innerHTML = `
    <p class="st-note">
      Sources are configured by the operator in <span class="st-mono">{data}/sources.json</span>.
      The public key there is the catalogue trust root; it cannot come from the index.
    </p>
    <div class="st-sources" data-sources></div>
    <div class="st-actions">
      <button type="button" data-refresh data-testid="store-sources-refresh">Refresh all</button>
      <button type="button" data-close data-testid="store-sources-close">Close</button>
    </div>
    <p class="stm-status" role="status" data-status data-testid="store-sources-status"></p>
  `;
  root.append(box);
  const list = box.querySelector("[data-sources]");
  const status = box.querySelector("[data-status]");
  let sources = [];
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "stm-status bad" : "stm-status";
  };
  function render() {
    list.replaceChildren();
    if (sources.length === 0) {
      const empty = document.createElement("div");
      empty.className = "st-empty";
      empty.textContent = "No source configured. Add one to {data}/sources.json and reopen this dialog.";
      list.append(empty);
      return;
    }
    for (const source of sources) {
      const row = document.createElement("div");
      row.className = "st-source";
      row.dataset.testid = "store-source";
      row.dataset.sourceId = source.id;
      row.innerHTML = `
        <strong>${escape(source.name)}</strong>
        <span class="st-badge ${source.trusted ? "ok" : "bad"}">${source.trusted ? "trusted" : "unverified"}</span>
        <span class="st-badge">${escape(source.kind)}</span>
        <span class="st-mono" title="${escape(source.url)}">${escape(source.id)}</span>
        <span class="st-note">${escape(age(source["refreshed-at"]))}</span>
      `;
      const refresh = document.createElement("button");
      refresh.type = "button";
      refresh.textContent = "Refresh";
      refresh.dataset.testid = "store-source-refresh";
      refresh.addEventListener("click", () => void refreshOne(source.id));
      row.append(refresh);
      list.append(row);
    }
  }
  async function load() {
    try {
      sources = await call(context, "/sources") ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function refreshOne(id) {
    try {
      setStatus(`Refreshing ${id}…`);
      await call(context, "/sources/refresh", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ source: id })
      });
      setStatus(`${id} refreshed.`);
      await load();
    } catch (error) {
      setStatus(`${id}: ${error}`, true);
    }
  }
  async function refreshAll() {
    try {
      setStatus("Refreshing every source…");
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
  box.querySelector("[data-refresh]")?.addEventListener("click", () => void refreshAll());
  box.querySelector("[data-close]")?.addEventListener("click", () => context.close());
  load();
}
function escape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
export {
  mount
};
