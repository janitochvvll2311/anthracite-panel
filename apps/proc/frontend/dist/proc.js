// src/api.ts
async function call(context, route, init = {}) {
  const { headers, ...rest } = init;
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}${route}`, { ...rest, headers: { accept: "application/json", ...headers ?? {} } });
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

// src/confirm.ts
function confirmAction(options) {
  return new Promise((resolve) => {
    const dialog = document.createElement("dialog");
    dialog.className = "confirm-dialog";
    const form = document.createElement("form");
    form.method = "dialog";
    form.className = "confirm-card";
    const title = document.createElement("h3");
    title.textContent = options.title;
    const message = document.createElement("p");
    message.textContent = options.message;
    const actions = document.createElement("div");
    actions.className = "confirm-actions";
    const cancel = document.createElement("button");
    cancel.type = "submit";
    cancel.value = "cancel";
    cancel.className = "confirm-cancel";
    cancel.textContent = "Cancel";
    const confirm = document.createElement("button");
    confirm.type = "submit";
    confirm.value = "confirm";
    confirm.className = options.danger ? "confirm-ok danger" : "confirm-ok";
    confirm.textContent = options.confirmLabel ?? "Confirm";
    actions.append(cancel, confirm);
    form.append(title, message, actions);
    dialog.append(form);
    dialog.addEventListener("close", () => {
      dialog.remove();
      resolve(dialog.returnValue === "confirm");
    });
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog)
        dialog.close("cancel");
    });
    document.body.append(dialog);
    dialog.showModal();
    cancel.focus();
  });
}

// src/format.ts
function memory(bytes) {
  if (bytes >= 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
  if (bytes >= 1024 * 1024)
    return `${Math.round(bytes / (1024 * 1024))} MiB`;
  if (bytes >= 1024)
    return `${Math.round(bytes / 1024)} KiB`;
  return `${bytes} B`;
}
function cell(className, text) {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
}
function reason(error) {
  return error instanceof Error ? error.message : String(error);
}

// src/styles.ts
var styles = `
  .proc { display: flex; flex-direction: column; gap: 10px; min-height: 380px; }
  .proc.fill { height: 100%; min-height: 0; padding: 14px 16px; }
  .proc-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .proc-count { color: #667087; font-size: 12px; }
  .proc-bar .grow { flex: 1; }
  .proc-bar button, .proc-bar select, .proc-spawn button, .proc-mobile-tools button, .proc-mobile-tools select {
    padding: 6px 10px; min-height: 34px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-bar button:hover, .proc-spawn button:hover { border-color: #3a4560; color: #fff; }
  .proc-tabs { display: inline-flex; border: 1px solid #2a3142; border-radius: 8px; overflow: hidden; }
  .proc-tabs button { border: 0; border-radius: 0; background: transparent; }
  .proc-tabs button[aria-pressed="true"] { background: #1f2940; color: #fff; }
  .proc-fold { font-size: 11px !important; }
  .proc-interval { display: flex; align-items: center; gap: 6px; color: #8f98ab; font-size: 12px; }
  .proc-interval select { padding: 5px 8px; min-height: 34px; }
  .proc-spawn { display: flex; gap: 6px; }
  .proc-spawn input {
    flex: 1; min-width: 0; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .proc-mobile-tools { display: none; }

  .proc-table {
    border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016;
    max-height: min(70vh, 760px);
  }
  .proc.fill .proc-table { flex: 1 1 auto; min-height: 0; max-height: none; }
  .proc-head { position: sticky; top: 0; z-index: 2; background: #0d1016; }
  .proc-body { min-width: 0; }
  .proc-body .proc-row { height: 40px; box-sizing: border-box; }
  .proc-spacer { pointer-events: none; }
  .proc-row {
    display: grid; gap: 8px; align-items: center; padding: 7px 10px;
    border-bottom: 1px solid #171c26; font-size: 12px;
  }
  .proc-row.flat { grid-template-columns: 64px 64px minmax(160px, 1.4fr) 90px 90px 100px 80px 66px; }
  .proc-row.tree { grid-template-columns: minmax(220px, 1fr) 90px 90px 100px 80px 66px; }
  .proc-row:last-child { border-bottom: 0; }
  .proc-row.head {
    background: #11151e; color: #667087;
    text-transform: uppercase; letter-spacing: .06em; font-size: 10px;
  }
  .proc-row.head .sortable { cursor: pointer; user-select: none; }
  .proc-row.head .sortable:focus-visible { outline: 2px solid #7aa2ff; outline-offset: 2px; border-radius: 3px; }
  .proc-row.head .sortable:hover { color: #c8d0df; }
  .proc-row.head .sorted { color: #e8ebf2; }
  .proc-row.filters { padding: 4px 10px; background: #10141c; border-bottom: 1px solid #202632; }
  .proc-filter-cell { min-width: 0; }
  .proc-filter-cell input {
    width: 100%; min-width: 0; padding: 3px 6px; border-radius: 5px; border: 1px solid #2a3142;
    background: #0b0e14; color: #e8ebf2; font: 11px ui-monospace, monospace;
  }
  .proc-filter-cell input::placeholder { color: #4d5668; }
  .proc-row:hover:not(.head):not(.filters) { background: #151a24; }
  .proc-row.flat .identity { display: contents; }
  .proc-row.tree .identity { display: flex; align-items: center; min-width: 0; }
  .proc-row .proc-meta { display: contents; }
  .proc-cell { min-width: 0; }
  .proc-pid, .proc-ppid {
    color: #8f98ab; font-family: ui-monospace, monospace; font-size: 11px;
  }
  .proc-row.tree .proc-pid { margin-left: 3px; margin-right: 8px; min-width: 3ch; }
  .proc-name {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #e8ebf2;
    min-width: 0; flex: 1;
  }
  .proc-gutter { display: flex; flex: none; align-self: stretch; }
  .proc-guide {
    position: relative; width: 16px; flex: none; align-self: stretch;
    margin-top: -7px; margin-bottom: -8px;
  }
  .proc-guide::before {
    content: ""; position: absolute; left: 7px; top: 0; bottom: 0;
    border-left: 1px solid #2a3142;
  }
  .proc-guide.blank::before { display: none; }
  .proc-connector {
    position: relative; width: 16px; flex: none; align-self: stretch;
    margin-top: -7px; margin-bottom: -8px;
  }
  .proc-connector::before {
    content: ""; position: absolute; left: 7px; top: 0; bottom: 0;
    border-left: 1px solid #2a3142;
  }
  .proc-connector.last::before { bottom: 50%; }
  .proc-connector::after {
    content: ""; position: absolute; left: 7px; top: 50%; width: 8px;
    border-top: 1px solid #2a3142;
  }
  .proc-twisty {
    width: 20px; height: 20px; padding: 0; flex: none; border: 0; background: transparent;
    color: #8f98ab; font: 11px/1 ui-monospace, monospace; cursor: pointer;
  }
  .proc-twisty:hover { color: #e8ebf2; }
  .proc-twisty.empty { visibility: hidden; }
  .proc-cell.user, .proc-cell.memory {
    color: #8f98ab; font-family: ui-monospace, monospace; font-size: 11px;
  }
  .proc-cell.state { text-transform: capitalize; }
  .proc-cell.state.running { color: #52d1a4; }
  .proc-cell.state.sleeping { color: #8f98ab; }
  .proc-cell.state.stopped { color: #f3c969; }
  .proc-cell.state.zombie { color: #ff7d8a; }
  .proc-bar-track { height: 5px; border-radius: 999px; background: #202634; overflow: hidden; flex: 1; }
  .proc-bar-fill { height: 100%; display: block; background: linear-gradient(90deg, #7aa2ff, #9d7bff); }
  .proc-cell.cpu { display: flex; align-items: center; gap: 6px; }
  .proc-cell.cpu .percent { width: 38px; text-align: right; color: #8f98ab; font: 11px ui-monospace, monospace; }
  .proc-cell[data-label]::before { content: none; }
  .proc-signal {
    padding: 4px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .proc-signal:hover { border-color: #ff7d8a88; color: #ffb3ba; }
  .proc-sentinel { height: 1px; }
  .proc-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .proc-status.bad { color: #ffb3ba; }
  .proc-empty { padding: 18px; color: #667087; text-align: center; font-size: 12px; }

  .proc.compact .proc-bar { gap: 8px; }
  .proc.compact .proc-count { flex: 1 0 100%; }
  .proc.compact .proc-bar .grow { display: none; }
  .proc.compact .proc-tabs { display: none; }
  .proc.compact .proc-interval { order: 1; }
  .proc.compact .proc-pick { order: 2; }
  .proc.compact [data-refresh] { order: 3; }
  .proc.compact .proc-fold { display: none; }
  .proc.compact .proc-mobile-tools {
    display: flex; gap: 6px; flex-wrap: wrap; align-items: center;
  }
  .proc.compact .proc-mobile-tools input {
    flex: 1 1 140px; min-width: 0; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .proc.compact .proc-mobile-tools select { flex: 0 0 auto; }
  .proc.compact .proc-direction { min-width: 54px; }
  .proc.compact .proc-spawn { width: 100%; }
  .proc.compact .proc-table { max-height: none; border: 0; background: transparent; overflow: visible; }
  .proc.compact.fill { padding: 10px; }
  .proc.compact.fill .proc-table { flex: 1 1 auto; min-height: 0; overflow: auto; }
  .proc.compact .proc-head { display: none; }
  .proc.compact .proc-body .proc-row { height: auto; }
  .proc.compact .proc-row.flat,
  .proc.compact .proc-row.tree {
    display: grid; grid-template-columns: minmax(0, 1fr) auto;
    grid-template-areas: "identity action" "meta action";
    gap: 6px 10px; margin-bottom: 8px; padding: 10px 12px;
    border: 1px solid #202632; border-radius: 10px; background: #0d1016;
  }
  .proc.compact .proc-row .identity {
    display: flex; grid-area: identity; min-width: 0; align-items: center;
    justify-content: flex-start; gap: 6px;
  }
  .proc.compact .proc-row .proc-pid::before { content: "PID "; color: #667087; }
  .proc.compact .proc-row .proc-ppid::before { content: "PPID "; color: #667087; }
  .proc.compact .proc-row .proc-name { margin-left: 2px; }
  .proc.compact .proc-row .proc-meta {
    display: flex; grid-area: meta; flex-wrap: wrap; align-items: center; gap: 2px 10px;
  }
  .proc.compact .proc-row .action { grid-area: action; align-self: start; }
  .proc.compact .proc-cell[data-label]::before {
    content: attr(data-label) " "; color: #667087; font-family: inherit;
    text-transform: none; letter-spacing: 0;
  }
  .proc.compact .proc-meta .ppid,
  .proc.compact .proc-meta .user,
  .proc.compact .proc-meta .state {
    border-left: 1px solid #202632; padding-left: 8px;
  }
  .proc.compact .proc-cell.cpu { flex: 1 0 100%; max-width: 240px; }
  .proc.compact .proc-name {
    white-space: normal; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical;
  }
  .proc.compact .proc-cell.action .proc-signal { min-height: 40px; padding: 8px 12px; font-size: 12px; }

  .confirm-dialog { border: 0; padding: 0; background: transparent; color: #e8ebf2; }
  .confirm-dialog::backdrop { background: rgba(5, 7, 11, .72); backdrop-filter: blur(2px); }
  .confirm-card {
    width: min(360px, calc(100vw - 32px)); display: flex; flex-direction: column; gap: 10px;
    padding: 16px; border: 1px solid #2a3142; border-radius: 12px; background: #11151e;
    box-shadow: 0 20px 60px #0008;
  }
  .confirm-card h3 { margin: 0; font-size: 15px; font-weight: 600; }
  .confirm-card p { margin: 0; color: #9aa4b8; font-size: 12.5px; line-height: 1.5; }
  .confirm-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .confirm-actions button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .confirm-actions .confirm-ok { border-color: #3a6ea5; background: #1d2b40; color: #dce9ff; }
  .confirm-actions .confirm-ok.danger { border-color: #7a3a44; background: #3a1c22; color: #ffb3ba; }
`;

// src/tree.ts
function filterProcesses(processes, filters, tree) {
  const has = (value) => Boolean(value && value.trim());
  const match = (value, needle) => value.toLowerCase().includes(needle.trim().toLowerCase());
  const matches = processes.filter((process) => {
    if (has(filters.pid) && !String(process.pid).includes(filters.pid.trim()))
      return false;
    if (has(filters.ppid) && !String(process.ppid).includes(filters.ppid.trim()))
      return false;
    if (has(filters.name) && !match(process.name, filters.name))
      return false;
    if (has(filters.user) && !match(process.user, filters.user))
      return false;
    if (has(filters.state) && !match(process.state, filters.state))
      return false;
    if (has(filters.query) && ![String(process.pid), String(process.ppid), process.name, process.user, process.state].some((value) => match(value, filters.query))) {
      return false;
    }
    return true;
  });
  if (!tree)
    return matches;
  const byPid = new Map(processes.map((process) => [process.pid, process]));
  const keep = new Set;
  for (const found of matches) {
    let current = found;
    while (current && !keep.has(current.pid)) {
      keep.add(current.pid);
      current = byPid.get(current.ppid);
    }
  }
  return processes.filter((process) => keep.has(process.pid));
}
function orderProcesses(processes, tree, collapsed, compareBy = (left, right) => left.pid - right.pid) {
  if (!tree) {
    return [...processes].sort(compareBy).map((process) => ({
      process,
      depth: 0,
      hasChildren: false,
      guides: [],
      isLast: true
    }));
  }
  const byPid = new Map(processes.map((process) => [process.pid, process]));
  const children = new Map;
  const roots = [];
  for (const process of processes) {
    const parent = byPid.get(process.ppid);
    if (parent && process.ppid !== process.pid) {
      const siblings = children.get(process.ppid) ?? [];
      siblings.push(process);
      children.set(process.ppid, siblings);
    } else {
      roots.push(process);
    }
  }
  const out = [];
  const seen = new Set;
  const reachable = new Set;
  const mark = (process) => {
    if (reachable.has(process.pid))
      return;
    reachable.add(process.pid);
    for (const child of children.get(process.pid) ?? [])
      mark(child);
  };
  for (const root of roots)
    mark(root);
  const walk = (process, depth, guides, isLast) => {
    if (seen.has(process.pid))
      return;
    seen.add(process.pid);
    const nested = [...children.get(process.pid) ?? []].sort(compareBy);
    out.push({
      process,
      depth,
      hasChildren: nested.length > 0,
      guides,
      isLast
    });
    if (collapsed.has(process.pid))
      return;
    nested.forEach((child, index) => {
      walk(child, depth + 1, [...guides, !isLast], index === nested.length - 1);
    });
  };
  roots.sort(compareBy).forEach((root, index) => walk(root, 0, [], index === roots.length - 1));
  for (const process of processes) {
    if (!reachable.has(process.pid))
      walk(process, 0, [], true);
  }
  return out;
}

// src/main.ts
function mount(root, context) {
  const filled = typeof context.fill === "function";
  if (filled) {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = filled ? "proc fill" : "proc";
  box.innerHTML = `
    <div class="proc-bar">
      <span class="proc-count">…</span>
      <div class="proc-tabs" role="group" aria-label="Layout">
        <button type="button" data-mode="flat" aria-pressed="true">List</button>
        <button type="button" data-mode="tree" aria-pressed="false">Tree</button>
      </div>
      <label class="proc-interval"><span class="proc-interval-label">Interval</span>
        <select data-interval aria-label="Refresh interval">
          <option value="0">Manual (0s)</option>
          <option value="1000">1s</option>
          <option value="2000" selected>2s</option>
          <option value="5000">5s</option>
          <option value="10000">10s</option>
        </select>
      </label>
      <button type="button" class="proc-fold" data-fold hidden>Collapse all</button>
      <span class="grow"></span>
      <select class="proc-pick" aria-label="Signal">
        <option value="TERM">TERM</option>
        <option value="INT">INT</option>
        <option value="HUP">HUP</option>
        <option value="STOP">STOP</option>
        <option value="CONT">CONT</option>
        <option value="KILL">KILL</option>
      </select>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="proc-mobile-tools">
      <input type="search" data-mobile-filter placeholder="Filter processes" aria-label="Filter processes" />
      <select data-mobile-sort aria-label="Sort by">
        <option value="pid">PID</option>
        <option value="ppid">PPID</option>
        <option value="name">Name</option>
        <option value="user">User</option>
        <option value="state">State</option>
        <option value="cpu">CPU</option>
        <option value="memory">Memory</option>
      </select>
      <button type="button" class="proc-direction" data-mobile-direction aria-label="Sort direction">Asc</button>
    </div>
    <form class="proc-spawn">
      <input name="command" aria-label="Detached command" placeholder="sleep 30" autocomplete="off" />
      <button type="submit">Spawn</button>
    </form>
    <div class="proc-table" role="table" aria-label="Processes">
      <div class="proc-head" data-head></div>
      <div class="proc-body" data-body role="rowgroup"></div>
      <div class="proc-sentinel" data-sentinel aria-hidden="true"></div>
    </div>
    <p class="proc-status" role="status"></p>
  `;
  root.append(box);
  const table = box.querySelector(".proc-table");
  const headRoot = box.querySelector("[data-head]");
  const bodyRoot = box.querySelector("[data-body]");
  const count = box.querySelector(".proc-count");
  const status = box.querySelector(".proc-status");
  const signal = box.querySelector(".proc-pick");
  const mobileFilter = box.querySelector("[data-mobile-filter]");
  const mobileSort = box.querySelector("[data-mobile-sort]");
  const mobileDirection = box.querySelector("[data-mobile-direction]");
  const sentinel = box.querySelector("[data-sentinel]");
  const fold = box.querySelector("[data-fold]");
  const modeButtons = [...box.querySelectorAll("[data-mode]")];
  const interval = box.querySelector("[data-interval]");
  const spawnForm = box.querySelector(".proc-spawn");
  const spawnInput = spawnForm.querySelector("input");
  const MOBILE_PAGE = 200;
  const DESKTOP_PAGE = 1e4;
  const ROW_HEIGHT = 40;
  const OVERSCAN = 8;
  let currentRows = [];
  let windowStart = -1;
  let windowEnd = -1;
  let scrollFrame = 0;
  const COMPACT_WIDTH = 820;
  let processes = [];
  let nextCursor = null;
  let sentinelVisible = false;
  let tree = false;
  let compact = false;
  let live = null;
  let intervalMs = 2000;
  let visible = true;
  let collapsed = new Set;
  let allCollapsed = false;
  let sortKey = "pid";
  let sortAsc = true;
  const filters = {};
  let loading = false;
  let pending = false;
  let disposed = false;
  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    scheduleLive();
    if (next && intervalMs > 0)
      load();
  });
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "proc-status bad" : "proc-status";
  };
  const updateDirection = () => {
    mobileDirection.textContent = sortAsc ? "Asc" : "Desc";
    mobileDirection.setAttribute("aria-label", sortAsc ? "Sort ascending" : "Sort descending");
    mobileSort.value = sortKey;
  };
  const compareBy = (left, right) => {
    let value = 0;
    switch (sortKey) {
      case "cpu":
        value = left["cpu-percent"] - right["cpu-percent"];
        break;
      case "memory":
        value = left["memory-bytes"] - right["memory-bytes"];
        break;
      case "name":
        value = left.name < right.name ? -1 : left.name > right.name ? 1 : 0;
        break;
      case "user":
        value = left.user < right.user ? -1 : left.user > right.user ? 1 : 0;
        break;
      case "state":
        value = left.state < right.state ? -1 : left.state > right.state ? 1 : 0;
        break;
      case "ppid":
        value = left.ppid - right.ppid;
        break;
      default:
        value = left.pid - right.pid;
    }
    if (value === 0)
      value = left.pid - right.pid;
    return sortAsc ? value : -value;
  };
  const sortable = (key) => {
    if (!key)
      return false;
    if (!tree)
      return true;
    return key === "user" || key === "state" || key === "cpu" || key === "memory";
  };
  const syncModeButtons = () => {
    for (const button of modeButtons) {
      button.setAttribute("aria-pressed", String(button.dataset.mode === (tree ? "tree" : "flat")));
    }
  };
  const filterCell = (key, placeholder) => {
    const wrapper = cell("proc-filter-cell", "");
    const input = document.createElement("input");
    input.type = "search";
    input.placeholder = placeholder;
    input.value = filters[key] ?? "";
    input.setAttribute("aria-label", `Filter by ${placeholder.toLowerCase()}`);
    input.addEventListener("input", () => {
      filters[key] = input.value;
      renderRows(true);
    });
    wrapper.append(input);
    return wrapper;
  };
  function renderHead() {
    headRoot.replaceChildren();
    const head = document.createElement("div");
    head.className = `proc-row head ${tree ? "tree" : "flat"}`;
    const labels = tree ? [
      ["PROCESS", ""],
      ["USER", "user"],
      ["STATE", "state"],
      ["CPU", "cpu"],
      ["MEMORY", "memory"],
      ["", ""]
    ] : [
      ["PID", "pid"],
      ["PPID", "ppid"],
      ["NAME", "name"],
      ["USER", "user"],
      ["STATE", "state"],
      ["CPU", "cpu"],
      ["MEMORY", "memory"],
      ["", ""]
    ];
    for (const [label, key] of labels) {
      const node = cell("", label);
      if (sortable(key)) {
        node.classList.add("sortable");
        if (sortKey === key) {
          node.classList.add("sorted");
          node.textContent = `${label} ${sortAsc ? "▲" : "▼"}`;
        }
        node.title = `Sort by ${label.toLowerCase()}`;
        node.tabIndex = 0;
        node.dataset.sort = key;
        node.setAttribute("role", "button");
        node.setAttribute("aria-label", `Sort by ${label.toLowerCase()}${sortKey === key ? sortAsc ? ", ascending" : ", descending" : ""}`);
        node.addEventListener("click", () => setSort(key));
        node.addEventListener("keydown", (event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setSort(key);
            headRoot.querySelector(`[data-sort="${key}"]`)?.focus();
          }
        });
      }
      head.append(node);
    }
    headRoot.append(head);
    const row = document.createElement("div");
    row.className = `proc-row filters ${tree ? "tree" : "flat"}`;
    if (tree) {
      row.append(filterCell("query", "PID or name"), filterCell("user", "User"), filterCell("state", "State"), cell("proc-filter-cell", ""), cell("proc-filter-cell", ""), cell("proc-filter-cell", ""));
    } else {
      row.append(filterCell("pid", "PID"), filterCell("ppid", "PPID"), filterCell("name", "Name"), filterCell("user", "User"), filterCell("state", "State"), cell("proc-filter-cell", ""), cell("proc-filter-cell", ""), cell("proc-filter-cell", ""));
    }
    headRoot.append(row);
  }
  function setSort(key) {
    if (key === sortKey)
      sortAsc = !sortAsc;
    else {
      sortKey = key;
      sortAsc = key === "pid" || key === "ppid" || key === "name" || key === "user" || key === "state";
    }
    updateDirection();
    renderHead();
    scheduleLive();
    renderRows(true);
  }
  const metaCell = (className, label, text) => {
    const node = cell(`proc-cell ${className}`, text);
    node.dataset.label = label;
    return node;
  };
  const cpuCell = (process) => {
    const cpu = document.createElement("span");
    cpu.className = "proc-cell cpu";
    cpu.dataset.label = "CPU";
    const track = document.createElement("span");
    track.className = "proc-bar-track";
    const fill = document.createElement("span");
    fill.className = "proc-bar-fill";
    fill.style.width = `${Math.min(100, Math.max(0, process["cpu-percent"]))}%`;
    track.append(fill);
    cpu.append(track, cell("percent", `${process["cpu-percent"].toFixed(1)}%`));
    cpu.title = `${process["cpu-percent"].toFixed(1)}% CPU`;
    return cpu;
  };
  const identityCell = (process, row) => {
    const identity = document.createElement("span");
    identity.className = "proc-cell identity";
    if (row) {
      const gutter = document.createElement("span");
      gutter.className = "proc-gutter";
      for (const line of row.guides) {
        const guide = document.createElement("span");
        guide.className = line ? "proc-guide" : "proc-guide blank";
        guide.setAttribute("aria-hidden", "true");
        gutter.append(guide);
      }
      if (row.depth > 0) {
        const connector = document.createElement("span");
        connector.className = row.isLast ? "proc-connector last" : "proc-connector";
        connector.setAttribute("aria-hidden", "true");
        gutter.append(connector);
      }
      identity.append(gutter);
      const twisty = document.createElement("button");
      twisty.type = "button";
      twisty.className = row.hasChildren ? "proc-twisty" : "proc-twisty empty";
      twisty.disabled = !row.hasChildren;
      twisty.tabIndex = row.hasChildren ? 0 : -1;
      twisty.textContent = collapsed.has(process.pid) ? "▸" : "▾";
      twisty.setAttribute("aria-label", collapsed.has(process.pid) ? "Expand children" : "Collapse children");
      twisty.setAttribute("aria-expanded", String(!collapsed.has(process.pid)));
      if (row.hasChildren) {
        twisty.addEventListener("click", () => {
          if (collapsed.has(process.pid))
            collapsed.delete(process.pid);
          else
            collapsed.add(process.pid);
          renderRows();
        });
      }
      identity.append(twisty);
    }
    const pid = cell("proc-pid", String(process.pid));
    pid.dataset.label = "PID";
    identity.append(pid);
    if (!row) {
      const ppid = cell("proc-ppid", String(process.ppid));
      ppid.dataset.label = "PPID";
      identity.append(ppid);
    }
    const name = cell("proc-name", process.name);
    name.title = process.name;
    identity.append(name);
    return identity;
  };
  function renderRow(rowData) {
    const process = rowData.process;
    const row = document.createElement("div");
    row.className = `proc-row ${tree ? "tree" : "flat"}`;
    row.append(identityCell(process, tree ? rowData : null));
    const meta = document.createElement("span");
    meta.className = "proc-meta";
    const state = cell(`proc-cell state ${process.state}`, process.state);
    state.dataset.label = "State";
    meta.append(metaCell("user", "User", process.user), state, cpuCell(process), metaCell("memory", "Memory", memory(process["memory-bytes"])));
    row.append(meta);
    const action = document.createElement("span");
    action.className = "proc-cell action";
    const send = document.createElement("button");
    send.type = "button";
    send.className = "proc-signal";
    send.textContent = "Send";
    send.setAttribute("aria-label", `Send signal to ${process.name} (${process.pid})`);
    send.addEventListener("click", () => {
      signalProcess(process.pid, process["start-ticks"], process.name);
    });
    action.append(send);
    row.append(action);
    return row;
  }
  function emptyCell() {
    return cell("proc-empty", Object.values(filters).some((value) => value && value.trim()) ? "No process matches the filters." : "No processes.");
  }
  function renderWindow(force = false) {
    if (compact) {
      const fragment = document.createDocumentFragment();
      for (const row of currentRows)
        fragment.append(renderRow(row));
      if (currentRows.length === 0)
        fragment.append(emptyCell());
      bodyRoot.replaceChildren(fragment);
      windowStart = -1;
      windowEnd = -1;
      return;
    }
    const headHeight = headRoot.offsetHeight;
    const viewport = Math.max(200, table.clientHeight - headHeight);
    const top = Math.max(0, table.scrollTop - headHeight);
    let start = Math.max(0, Math.floor(top / ROW_HEIGHT) - OVERSCAN);
    start = Math.min(start, currentRows.length);
    const end = Math.min(currentRows.length, Math.ceil((top + viewport) / ROW_HEIGHT) + OVERSCAN);
    if (!force && start === windowStart && end === windowEnd)
      return;
    windowStart = start;
    windowEnd = end;
    const fragment = document.createDocumentFragment();
    if (start > 0) {
      const spacer = document.createElement("div");
      spacer.className = "proc-spacer";
      spacer.style.height = `${start * ROW_HEIGHT}px`;
      fragment.append(spacer);
    }
    for (let index = start;index < end; index += 1) {
      fragment.append(renderRow(currentRows[index]));
    }
    if (end < currentRows.length) {
      const spacer = document.createElement("div");
      spacer.className = "proc-spacer";
      spacer.style.height = `${(currentRows.length - end) * ROW_HEIGHT}px`;
      fragment.append(spacer);
    }
    if (currentRows.length === 0)
      fragment.append(emptyCell());
    bodyRoot.replaceChildren(fragment);
  }
  function renderRows(resetScroll = false) {
    const visible = filterProcesses(processes, filters, tree);
    currentRows = orderProcesses(visible, tree, collapsed, compareBy);
    if (resetScroll)
      table.scrollTop = 0;
    renderWindow(true);
    const active = Object.values(filters).some((value) => value && value.trim());
    const countText = active ? `${currentRows.length} / ${processes.length}` : `${processes.length}`;
    count.textContent = `${countText} process${processes.length === 1 ? "" : "es"}${tree ? " · tree" : ""}`;
  }
  async function loadAll() {
    const all = [];
    const seen = new Set;
    let cursor = null;
    while (true) {
      const suffix = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
      const page = await call(context, `/processes?limit=${DESKTOP_PAGE}${suffix}`);
      all.push(...page.processes);
      if (disposed || page.done || !page.cursor || seen.has(page.cursor))
        break;
      seen.add(page.cursor);
      cursor = page.cursor;
    }
    return all;
  }
  async function loadTree() {
    const all = await loadAll();
    if (disposed)
      return;
    processes = all;
    nextCursor = null;
    fold.hidden = false;
    renderRows(true);
    setStatus(`Tree snapshot · ${processes.length} processes`);
  }
  async function load(cursor = null, append = false) {
    if (loading) {
      pending = true;
      return;
    }
    loading = true;
    setStatus(append ? "Loading more processes…" : "Reading the process table…");
    try {
      if (tree) {
        await loadTree();
        return;
      }
      if (compact) {
        const limit = append ? MOBILE_PAGE : Math.max(MOBILE_PAGE, processes.length);
        const suffix = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
        const page = await call(context, `/processes?limit=${limit}${suffix}`);
        if (disposed)
          return;
        if (append)
          processes.push(...page.processes);
        else
          processes = page.processes;
        nextCursor = page.cursor ?? null;
      } else {
        processes = await loadAll();
        if (disposed)
          return;
        nextCursor = null;
      }
      fold.hidden = true;
      renderRows(false);
      setStatus(`Snapshot updated · ${processes.length} processes`);
    } catch (error) {
      if (!disposed)
        setStatus(reason(error), true);
    } finally {
      loading = false;
      if (pending) {
        pending = false;
        load();
      }
    }
  }
  function moreIfNeeded() {
    if (!compact || tree || loading || !nextCursor)
      return;
    load(nextCursor, true);
  }
  async function signalProcess(pid, startTicks, name) {
    if (!signal)
      return;
    const chosen = signal.value;
    const dangerous = chosen === "TERM" || chosen === "KILL" || chosen === "STOP";
    const ok = await confirmAction({
      title: `Send ${chosen} to ${name} (${pid})?`,
      message: dangerous ? "The process may stop and unsaved work can be lost." : "The process will receive the signal.",
      confirmLabel: `Send ${chosen}`,
      danger: dangerous
    });
    if (!ok)
      return;
    setStatus(`Sending ${chosen} to ${pid}…`);
    try {
      await call(context, "/signal", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ pid, "start-ticks": startTicks, signal: chosen })
      });
      setStatus(`${chosen} sent to ${pid}`);
      await load();
    } catch (error) {
      if (!disposed)
        setStatus(reason(error), true);
    }
  }
  function scheduleLive() {
    if (live !== null)
      window.clearInterval(live);
    live = null;
    if (!visible || intervalMs === 0)
      return;
    live = window.setInterval(() => {
      if (!document.hidden)
        load();
    }, intervalMs);
  }
  function setMode(next) {
    if (next && compact)
      return;
    if (next) {
      filters.pid = "";
      filters.ppid = "";
      filters.name = "";
    } else {
      filters.query = "";
    }
    tree = next;
    if (!sortable(sortKey)) {
      sortKey = "pid";
      sortAsc = true;
    }
    syncModeButtons();
    fold.hidden = !tree;
    allCollapsed = false;
    fold.textContent = "Collapse all";
    renderHead();
    updateDirection();
    table.scrollTop = 0;
    scheduleLive();
    load();
  }
  function applyCompact() {
    const next = box.clientWidth < COMPACT_WIDTH;
    if (next === compact)
      return;
    compact = next;
    box.classList.toggle("compact", compact);
    if (compact) {
      filters.pid = "";
      filters.ppid = "";
      filters.name = "";
      filters.user = "";
      filters.state = "";
      mobileFilter.value = filters.query ?? "";
      processes = [];
      nextCursor = null;
      if (tree) {
        tree = false;
        syncModeButtons();
        fold.hidden = true;
      }
    } else {
      filters.query = "";
      mobileFilter.value = "";
    }
    renderHead();
    table.scrollTop = 0;
    scheduleLive();
    load();
  }
  for (const button of modeButtons) {
    button.addEventListener("click", () => setMode(button.dataset.mode === "tree"));
  }
  fold.addEventListener("click", () => {
    allCollapsed = !allCollapsed;
    collapsed = new Set;
    if (allCollapsed) {
      const parents = new Set(processes.map((process) => process.ppid));
      for (const process of processes) {
        if (parents.has(process.pid))
          collapsed.add(process.pid);
      }
    }
    fold.textContent = allCollapsed ? "Expand all" : "Collapse all";
    renderRows(true);
  });
  interval.addEventListener("change", () => {
    intervalMs = Number(interval.value) || 0;
    scheduleLive();
    load();
  });
  mobileFilter.addEventListener("input", () => {
    filters.query = mobileFilter.value;
    renderRows(true);
  });
  mobileSort.addEventListener("change", () => {
    setSort(mobileSort.value);
  });
  mobileDirection.addEventListener("click", () => {
    sortAsc = !sortAsc;
    updateDirection();
    renderHead();
    renderRows(true);
  });
  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  spawnForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const command = spawnInput.value.trim();
    if (!command) {
      setStatus("Write a command first, for example `sleep 30`.", true);
      return;
    }
    const argv = command.split(/\s+/);
    const ok = await confirmAction({
      title: "Spawn this command?",
      message: `\`${command}\` will run detached from the process service.`,
      confirmLabel: "Spawn"
    });
    if (!ok)
      return;
    setStatus("Spawning…");
    try {
      const pid = await call(context, "/spawn", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ argv })
      });
      spawnInput.value = "";
      await load();
      setStatus(`Spawned ${argv[0]} as pid ${pid}`);
    } catch (error) {
      if (!disposed)
        setStatus(reason(error), true);
    }
  });
  const resize = new ResizeObserver(applyCompact);
  resize.observe(box);
  const tableResize = new ResizeObserver(() => renderWindow(false));
  tableResize.observe(table);
  const sentinelObserver = new IntersectionObserver(([entry]) => {
    sentinelVisible = entry.isIntersecting;
    if (sentinelVisible)
      moreIfNeeded();
  }, { rootMargin: "160px" });
  sentinelObserver.observe(sentinel);
  applyCompact();
  renderHead();
  updateDirection();
  scheduleLive();
  table.addEventListener("scroll", () => {
    if (compact || scrollFrame)
      return;
    scrollFrame = window.requestAnimationFrame(() => {
      scrollFrame = 0;
      renderWindow(false);
    });
  });
  load();
  return () => {
    disposed = true;
    offVisibility?.();
    if (live !== null)
      window.clearInterval(live);
    if (scrollFrame)
      window.cancelAnimationFrame(scrollFrame);
    resize.disconnect();
    tableResize.disconnect();
    sentinelObserver.disconnect();
    root.replaceChildren();
  };
}
export {
  mount
};
