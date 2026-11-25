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
function createSseParser(onEvent) {
  let buffer = "";
  return (chunk) => {
    buffer += chunk.replace(/\r/g, "");
    for (;; ) {
      const end = buffer.indexOf(`

`);
      if (end < 0)
        break;
      const block = buffer.slice(0, end);
      buffer = buffer.slice(end + 2);
      const data = block.split(`
`).filter((line) => line.startsWith("data:")).map((line) => line.slice(5).replace(/^ /, "")).join(`
`);
      if (data)
        onEvent(data);
    }
  };
}
async function readBytes(response, signal, onChunk) {
  if (!response.body)
    throw new Error("the stream has no body");
  const reader = response.body.getReader();
  const cancel = () => void reader.cancel();
  if (signal.aborted) {
    reader.cancel();
    return;
  }
  signal.addEventListener("abort", cancel, { once: true });
  try {
    for (;; ) {
      const { done, value } = await reader.read();
      if (done)
        break;
      onChunk(value);
    }
  } finally {
    signal.removeEventListener("abort", cancel);
  }
}
async function watchLogs(context, id, signal, onText) {
  const response = await context.fetch(route(context, `/container/logs?id=${encodeURIComponent(id)}&tail=400&follow=1`), { headers: { accept: "application/octet-stream" }, signal });
  if (!response.ok) {
    throw new Error(await response.text() || `HTTP ${response.status}`);
  }
  const decoder = new TextDecoder;
  await readBytes(response, signal, (bytes) => {
    onText(decoder.decode(bytes, { stream: true }));
  });
}
async function watchStats(context, id, signal, onSample) {
  const response = await context.fetch(route(context, `/container/stats?id=${encodeURIComponent(id)}`), { headers: { accept: "text/event-stream" }, signal });
  if (!response.ok) {
    throw new Error(await response.text() || `HTTP ${response.status}`);
  }
  if (!response.body)
    throw new Error("the stats stream has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder;
  const feed = createSseParser((data) => {
    try {
      onSample(JSON.parse(data));
    } catch {}
  });
  for (;; ) {
    const { done, value } = await reader.read();
    if (done)
      break;
    feed(decoder.decode(value, { stream: true }));
  }
}
async function watchExec(context, session, signal, onText) {
  const response = await context.fetch(route(context, `/container/exec/attach?session=${encodeURIComponent(session)}`), { headers: { accept: "application/octet-stream" }, signal });
  if (!response.ok) {
    throw new Error(await response.text() || `HTTP ${response.status}`);
  }
  const decoder = new TextDecoder;
  await readBytes(response, signal, (bytes) => {
    onText(decoder.decode(bytes, { stream: true }));
  });
}

// src/state.ts
function stateKind(state) {
  if (state === "running")
    return "ok";
  if (state === "paused")
    return "warn";
  if (state === "restarting")
    return "warn";
  if (state === "exited" || state === "dead")
    return "bad";
  return "off";
}
function shortId(id) {
  return id.length > 12 ? id.slice(0, 12) : id;
}
function formatBytes(bytes) {
  if (!bytes)
    return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}
function percent(value) {
  return `${value.toFixed(1)}%`;
}

// src/styles.ts
var styles = `
  .cn { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .cn-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .cn-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .cn-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .cn button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cn button:hover { border-color: var(--accent); }
  .cn button.primary { border-color: var(--accent); background: var(--panel-3); }
  .cn button.danger { border-color: var(--danger); color: var(--danger); }
  .cn button:disabled { opacity: .55; cursor: default; }
  .cn input[type="text"], .cn input[type="search"], .cn select {
    padding: 5px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 12px;
  }
  .cn-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .cn-status.bad { color: var(--danger); }
  .cn-tabs { display: flex; gap: 4px; flex-wrap: wrap; }
  .cn-tab {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--muted); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cn-tab.active { border-color: var(--accent); background: var(--panel-2); color: var(--text); }
  .cn-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(290px, 1fr)); gap: 10px; }
  .cn-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0;
    cursor: pointer;
  }
  .cn-card:hover { border-color: var(--line); }
  .cn-card.selected { border-color: var(--accent); }
  .cn-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .cn-card-head strong {
    font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cn-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .cn-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .cn-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .cn-badge.ok { border-color: var(--good); color: var(--good); }
  .cn-badge.warn { border-color: var(--warn); color: var(--warn); }
  .cn-badge.bad { border-color: var(--danger); color: var(--danger); }
  .cn-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px;
    color: var(--muted); font-size: 12px;
  }
  .cn-banner {
    padding: 8px 10px; border-radius: 8px; border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, var(--panel)); color: var(--danger); font-size: 12px;
  }
  .cn-note { font-size: 12px; color: var(--muted); }
  .cn-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .cn-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .cn-detail-head strong { font-size: 13px; color: var(--text); }
  .cn-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .cn-facts { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 8px; }
  .cn-strip { display: flex; gap: 10px; flex-wrap: wrap; }
  .cn-strip span { font-size: 12px; color: var(--muted); }
  .cn-strip strong { color: var(--text); font-weight: 600; }
  .cn-log {
    margin: 0; padding: 10px; border-radius: 8px; border: 1px solid var(--line-soft);
    background: var(--bg); color: var(--text); font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11.5px; line-height: 1.45; max-height: 320px; overflow: auto; white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cn-dialog {
    border: 1px solid var(--line); border-radius: 12px; padding: 14px;
    background: var(--panel); color: var(--text); width: min(560px, calc(100vw - 32px));
  }
  .cn-dialog::backdrop { background: #05070bb3; }
  .cn-dialog h3 { margin: 0 0 10px; font-size: 13px; }
  .cn-dialog label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--muted); }
  .cn-dialog .cn-actions { justify-content: flex-end; margin-top: 12px; }
  .cn-exec-out {
    margin: 0 0 8px; padding: 8px; min-height: 180px; max-height: 300px; overflow: auto;
    border-radius: 8px; border: 1px solid var(--line-soft); background: var(--bg);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px;
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .cn-exec-line { display: flex; gap: 6px; }
  .cn-exec-line input { flex: 1; }

  html[data-viewport="tablet"] .cn-grid { grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); }
  html[data-viewport="mobile"] .cn-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .cn-actions { width: 100%; }
  html[data-viewport="mobile"] .cn-actions button { flex: 1 1 auto; }

  @container (max-width: 640px) {
    .cn-grid { grid-template-columns: minmax(0, 1fr); }
    .cn-actions button { flex: 1 1 auto; }
  }
`;

// src/main.ts
function date(seconds) {
  if (!seconds)
    return "—";
  return new Date(seconds * 1000).toLocaleString();
}
function fillStrip(strip, sample) {
  const cpu = strip.querySelector("[data-cpu]");
  const memory = strip.querySelector("[data-memory]");
  const network = strip.querySelector("[data-network]");
  const pids = strip.querySelector("[data-pids]");
  if (cpu)
    cpu.textContent = percent(sample["cpu-percent"] ?? 0);
  if (memory)
    memory.textContent = `${formatBytes(sample["memory-bytes"] ?? 0)} / ${formatBytes(sample["memory-limit"] ?? 0)}`;
  if (network)
    network.textContent = `↓ ${formatBytes(sample["net-rx"] ?? 0)} ↑ ${formatBytes(sample["net-tx"] ?? 0)}`;
  if (pids)
    pids.textContent = String(sample.pids ?? 0);
}
function escape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = "cn";
  box.dataset.testid = "containers-view";
  box.innerHTML = `
    <div class="cn-bar">
      <span class="cn-title">Containers</span>
      <span class="cn-badge" data-counts></span>
      <span class="grow"></span>
      <button type="button" data-create data-testid="containers-create">Create</button>
      <button type="button" data-refresh data-testid="containers-refresh">Refresh</button>
    </div>
    <p class="cn-status" role="status" data-status data-testid="containers-status"></p>
    <div class="cn-tabs" role="tablist">
      <button type="button" role="tab" class="cn-tab active" data-tab="containers" data-testid="containers-tab-containers">Containers</button>
      <button type="button" role="tab" class="cn-tab" data-tab="images" data-testid="containers-tab-images">Images</button>
      <button type="button" role="tab" class="cn-tab" data-tab="volumes" data-testid="containers-tab-volumes">Volumes</button>
      <button type="button" role="tab" class="cn-tab" data-tab="networks" data-testid="containers-tab-networks">Networks</button>
    </div>
    <div class="cn-grid" data-containers data-testid="containers-list"></div>
    <section class="cn-detail" data-detail data-testid="containers-detail" hidden></section>
    <section data-panel-images hidden>
      <div class="cn-bar">
        <input type="text" data-pull-reference placeholder="alpine:3.21" data-testid="containers-pull-reference" />
        <button type="button" data-pull data-testid="containers-pull">Pull image</button>
      </div>
      <div class="cn-grid" data-images></div>
    </section>
    <section data-panel-volumes hidden>
      <div class="cn-bar">
        <input type="text" data-volume-name placeholder="data" data-testid="containers-volume-name" />
        <button type="button" data-volume-create data-testid="containers-volume-create">Create volume</button>
      </div>
      <div class="cn-grid" data-volumes></div>
    </section>
    <section data-panel-networks hidden>
      <div class="cn-grid" data-networks></div>
    </section>
    <dialog class="cn-dialog" data-confirm>
      <h3 data-confirm-title>Confirm</h3>
      <p class="cn-note" data-confirm-message></p>
      <label data-force hidden><span><input type="checkbox" data-confirm-force /> force a running container</span></label>
      <div class="cn-actions">
        <button type="button" data-confirm-cancel data-testid="containers-confirm-cancel">Cancel</button>
        <button type="button" class="danger" data-confirm-run data-testid="containers-confirm-run">Confirm</button>
      </div>
    </dialog>
    <dialog class="cn-dialog" data-exec-dialog>
      <h3 data-exec-title>Exec</h3>
      <pre class="cn-exec-out" data-exec-out data-testid="containers-exec-out"></pre>
      <div class="cn-exec-line">
        <input type="text" data-exec-input data-testid="containers-exec-input" placeholder="command and Enter" />
        <button type="button" data-exec-send data-testid="containers-exec-send">Send</button>
      </div>
      <div class="cn-actions">
        <button type="button" data-exec-close data-testid="containers-exec-close">Close</button>
      </div>
    </dialog>
  `;
  root.append(box);
  const status = box.querySelector("[data-status]");
  const counts = box.querySelector("[data-counts]");
  const list = box.querySelector("[data-containers]");
  const detail = box.querySelector("[data-detail]");
  const imagesNode = box.querySelector("[data-images]");
  const volumesNode = box.querySelector("[data-volumes]");
  const networksNode = box.querySelector("[data-networks]");
  const confirmDialog = box.querySelector("[data-confirm]");
  const execDialog = box.querySelector("[data-exec-dialog]");
  const execOut = box.querySelector("[data-exec-out]");
  const execInput = box.querySelector("[data-exec-input]");
  const pullReference = box.querySelector("[data-pull-reference]");
  const volumeName = box.querySelector("[data-volume-name]");
  const createButton = box.querySelector("[data-create]");
  const hasFab = context.panel?.slots?.includes("panel.fab") === true;
  const hasModal = context.panel?.slots?.includes("panel.modal") === true;
  if (hasFab || !hasModal)
    createButton.hidden = true;
  let containers = [];
  let images = [];
  let volumes = [];
  let networks = [];
  let selected = null;
  let tab = "containers";
  let logs = null;
  let stats = null;
  let exec = null;
  let execSession = null;
  let visible = true;
  let disposed = false;
  let pending = null;
  let logBuffer = "";
  let statSample = null;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "cn-status bad" : "cn-status";
  };
  const badge = (kind, text) => `<span class="cn-badge ${kind}">${escape(text)}</span>`;
  function renderCounts() {
    const running = containers.filter((entry) => entry.state === "running").length;
    counts.textContent = `${containers.length} containers · ${running} running`;
  }
  function renderContainers() {
    list.replaceChildren();
    if (containers.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent = "No container in this runtime. Use the FAB or the CLI to create one.";
      list.append(empty);
      return;
    }
    for (const container of containers) {
      const card = document.createElement("div");
      card.className = `cn-card${container.id === selected ? " selected" : ""}`;
      card.dataset.testid = "container";
      card.dataset.containerId = container.id;
      card.dataset.state = container.state;
      card.innerHTML = `
        <div class="cn-card-head">
          <strong title="${escape(container.name)}">${escape(container.name)}</strong>
          <span class="grow"></span>
          ${container.health ? badge(container.health === "healthy" ? "ok" : "warn", container.health) : ""}
          <span class="cn-badge ${stateKind(container.state)}" data-testid="container-state">${escape(container.state)}</span>
        </div>
        <div class="cn-field"><span class="cn-field-name">Image</span><span class="cn-mono" title="${escape(container.image)}">${escape(container.image)}</span></div>
        <div class="cn-field"><span class="cn-field-name">Status</span><span>${escape(container.status || "—")}</span></div>
        <div class="cn-field"><span class="cn-field-name">Ports</span><span class="cn-mono">${escape(container.ports.join(", ") || "—")}</span></div>
      `;
      card.addEventListener("click", () => select(container.id));
      list.append(card);
    }
  }
  function renderDetail() {
    const container = containers.find((entry) => entry.id === selected);
    if (!container) {
      detail.hidden = true;
      detail.replaceChildren();
      return;
    }
    detail.hidden = false;
    detail.replaceChildren();
    const head = document.createElement("div");
    head.className = "cn-detail-head";
    head.innerHTML = `
      <strong>${escape(container.name)}</strong>
      ${badge(stateKind(container.state), container.state)}
      <span class="cn-mono">${escape(shortId(container.id))}</span>
    `;
    const facts = document.createElement("div");
    facts.className = "cn-facts";
    const fact = (name, value) => `<div class="cn-field"><span class="cn-field-name">${escape(name)}</span><span>${escape(value)}</span></div>`;
    facts.innerHTML = [
      fact("Image", container.image),
      fact("Created", date(container["created-at"])),
      fact("Ports", container.ports.join(", ") || "—"),
      fact("Labels", container.labels.map((label) => `${label.name}=${label.value}`).join(", ") || "—")
    ].join("");
    const actions = document.createElement("div");
    actions.className = "cn-actions";
    const action = (label, testid, run, kind = "") => {
      const button = document.createElement("button");
      button.type = "button";
      if (kind)
        button.className = kind;
      button.textContent = label;
      button.dataset.testid = testid;
      button.addEventListener("click", run);
      actions.append(button);
    };
    if (container.state === "running") {
      action("Stop", "container-stop", () => void lifecycle(container, "stop"));
      action("Restart", "container-restart", () => void lifecycle(container, "restart"));
      action("Pause", "container-pause", () => void lifecycle(container, "pause"));
    } else if (container.state === "paused") {
      action("Unpause", "container-unpause", () => void lifecycle(container, "unpause"));
      action("Stop", "container-stop", () => void lifecycle(container, "stop"));
    } else if (container.state === "restarting") {
      action("Stop", "container-stop", () => void lifecycle(container, "stop"));
    } else {
      action("Start", "container-start", () => void lifecycle(container, "start"));
    }
    action("Logs", "container-logs", () => void toggleLogs(container));
    action("Exec", "container-exec", () => void openExec(container));
    action("Remove", "container-remove", () => openRemove(container), "danger");
    action("Close", "container-close", () => {
      selected = null;
      stopDetailStreams();
      render();
    });
    const strip = document.createElement("div");
    strip.className = "cn-strip";
    strip.dataset.testid = "containers-stats";
    strip.innerHTML = `<span>CPU <strong data-cpu>—</strong></span><span>Memory <strong data-memory>—</strong></span><span>Network <strong data-network>—</strong></span><span>PIDs <strong data-pids>—</strong></span>`;
    if (statSample)
      fillStrip(strip, statSample);
    const log = document.createElement("pre");
    log.className = "cn-log";
    log.dataset.testid = "containers-log";
    log.textContent = logBuffer || `Select Logs to follow this container.
`;
    detail.append(head, facts, actions, strip, log);
  }
  function renderImages() {
    imagesNode.replaceChildren();
    if (images.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent = "No image in this runtime.";
      imagesNode.append(empty);
      return;
    }
    for (const image of images) {
      const card = document.createElement("div");
      card.className = "cn-card";
      card.dataset.testid = "image";
      card.dataset.imageId = image.id;
      card.innerHTML = `
        <div class="cn-card-head"><strong>${escape(image.tags[0] ?? shortId(image.id))}</strong></div>
        <div class="cn-field"><span class="cn-field-name">Id</span><span class="cn-mono">${escape(shortId(image.id))}</span></div>
        <div class="cn-field"><span class="cn-field-name">Size</span><span>${escape(formatBytes(image.size))}</span></div>
        <div class="cn-field"><span class="cn-field-name">Digest</span><span class="cn-mono">${escape(image.digest ?? "—")}</span></div>
      `;
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "danger";
      remove.textContent = "Remove";
      remove.dataset.testid = "image-remove";
      remove.addEventListener("click", () => openConfirm(`Remove image ${image.tags[0] ?? shortId(image.id)}?`, "Remove", false, async () => {
        await call(context, "/images/remove", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id: image.id, force: false })
        });
        await loadImages();
      }));
      card.append(remove);
      imagesNode.append(card);
    }
  }
  function renderVolumes() {
    volumesNode.replaceChildren();
    if (volumes.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent = "No named volume.";
      volumesNode.append(empty);
      return;
    }
    for (const volume of volumes) {
      const card = document.createElement("div");
      card.className = "cn-card";
      card.dataset.testid = "volume";
      card.dataset.volumeName = volume.name;
      card.innerHTML = `
        <div class="cn-card-head"><strong>${escape(volume.name)}</strong></div>
        <div class="cn-field"><span class="cn-field-name">Driver</span><span>${escape(volume.driver)}</span></div>
      `;
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "danger";
      remove.textContent = "Remove";
      remove.dataset.testid = "volume-remove";
      remove.addEventListener("click", () => openConfirm(`Remove volume ${volume.name}?`, "Remove", false, async () => {
        await call(context, "/volumes/remove", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: volume.name, force: false })
        });
        await loadVolumes();
      }));
      card.append(remove);
      volumesNode.append(card);
    }
  }
  function renderNetworks() {
    networksNode.replaceChildren();
    if (networks.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent = "No network in this runtime.";
      networksNode.append(empty);
      return;
    }
    for (const network of networks) {
      const card = document.createElement("div");
      card.className = "cn-card";
      card.dataset.testid = "network";
      card.innerHTML = `
        <div class="cn-card-head"><strong>${escape(network.name)}</strong></div>
        <div class="cn-field"><span class="cn-field-name">Driver</span><span>${escape(network.driver)}</span></div>
        <div class="cn-field"><span class="cn-field-name">Subnet</span><span class="cn-mono">${escape(network.subnet ?? "—")}</span></div>
        <div class="cn-field"><span class="cn-field-name">Containers</span><span>${network.containers}</span></div>
      `;
      networksNode.append(card);
    }
  }
  function render() {
    renderCounts();
    renderContainers();
    renderDetail();
    renderImages();
    renderVolumes();
    renderNetworks();
    for (const button of box.querySelectorAll(".cn-tab")) {
      const current = button.dataset.tab === tab;
      button.classList.toggle("active", current);
      button.setAttribute("aria-selected", String(current));
    }
  }
  async function loadContainers() {
    try {
      containers = await call(context, "/containers") ?? [];
      if (selected && !containers.some((entry) => entry.id === selected)) {
        selected = null;
        stopDetailStreams();
      }
      setStatus("");
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function loadImages() {
    try {
      images = await call(context, "/images") ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function loadVolumes() {
    try {
      volumes = await call(context, "/volumes") ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function loadNetworks() {
    try {
      networks = await call(context, "/networks") ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  function switchTab(next) {
    tab = next;
    box.querySelector("[data-containers]").hidden = next !== "containers";
    detail.hidden = next !== "containers" || !selected;
    for (const name of ["images", "volumes", "networks"]) {
      box.querySelector(`[data-panel-${name}]`).hidden = next !== name;
    }
    if (next === "images")
      loadImages();
    else if (next === "volumes")
      loadVolumes();
    else if (next === "networks")
      loadNetworks();
    render();
  }
  function select(id) {
    if (selected === id)
      return;
    selected = id;
    stopDetailStreams();
    logBuffer = "";
    statSample = null;
    render();
    startStats();
  }
  function stopDetailStreams() {
    logs?.abort();
    logs = null;
    stats?.abort();
    stats = null;
    closeExec();
  }
  function startStats() {
    if (!selected)
      return;
    stats?.abort();
    const container = containers.find((entry) => entry.id === selected);
    if (!container)
      return;
    const strip = detail.querySelector(".cn-strip");
    if (!strip)
      return;
    const controller = new AbortController;
    stats = controller;
    watchStats(context, container.id, controller.signal, (sample) => {
      statSample = sample;
      const current = detail.querySelector(".cn-strip");
      if (current)
        fillStrip(current, sample);
    }).catch(() => {
      if (!controller.signal.aborted)
        setStatus("Stats stopped; the runtime ended the stream.", true);
    });
  }
  async function toggleLogs(container) {
    const log = detail.querySelector(".cn-log");
    if (!log)
      return;
    if (logs) {
      logs.abort();
      logs = null;
      setStatus("Logs stopped.");
      return;
    }
    log.textContent = "";
    const controller = new AbortController;
    logs = controller;
    try {
      await watchLogs(context, container.id, controller.signal, (text) => {
        logBuffer += text;
        if (logBuffer.length > 200000)
          logBuffer = logBuffer.slice(-150000);
        const current = detail.querySelector(".cn-log");
        if (current) {
          current.textContent = logBuffer;
          current.scrollTop = current.scrollHeight;
        }
      });
    } catch (error) {
      if (!controller.signal.aborted)
        setStatus(String(error), true);
    } finally {
      if (logs === controller)
        logs = null;
    }
  }
  async function lifecycle(container, action) {
    try {
      setStatus(`${action} ${container.name}…`);
      await call(context, `/container/${action}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id: container.id, "timeout-s": 10 })
      });
      setStatus(`${container.name}: ${action} requested.`);
      await loadContainers();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  function openRemove(container) {
    openConfirm(`Remove ${container.name}? Named volumes are kept.`, "Remove", true, async () => {
      const force = confirmDialog.querySelector("[data-confirm-force]").checked;
      await call(context, "/container/remove", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id: container.id, force })
      });
      selected = null;
      stopDetailStreams();
      await loadContainers();
    });
  }
  function openConfirm(message, label, force, run) {
    pending = run;
    confirmDialog.querySelector("[data-confirm-title]").textContent = label;
    confirmDialog.querySelector("[data-confirm-message]").textContent = message;
    confirmDialog.querySelector("[data-confirm-run]").textContent = label;
    const forceLabel = confirmDialog.querySelector("[data-force]");
    forceLabel.hidden = !force;
    const checkbox = confirmDialog.querySelector("[data-confirm-force]");
    checkbox.checked = false;
    confirmDialog.showModal();
  }
  async function openExec(container) {
    try {
      const session = await call(context, "/container/exec", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          id: container.id,
          command: ["/bin/sh"],
          tty: true,
          cols: 100,
          rows: 30
        })
      });
      execSession = String(session);
      execOut.textContent = "";
      execInput.value = "";
      execDialog.showModal();
      execInput.focus();
      const controller = new AbortController;
      exec = controller;
      watchExec(context, execSession, controller.signal, (text) => {
        execOut.textContent = (execOut.textContent ?? "") + text;
        execOut.scrollTop = execOut.scrollHeight;
      }).catch(() => {
        if (!controller.signal.aborted)
          setStatus("Exec stream ended.", true);
      });
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function sendExec() {
    if (!execSession)
      return;
    const text = `${execInput.value}
`;
    execInput.value = "";
    try {
      await call(context, "/container/exec-write", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          session: execSession,
          data: Array.from(new TextEncoder().encode(text))
        })
      });
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  function closeExec() {
    exec?.abort();
    exec = null;
    if (execSession) {
      const session = execSession;
      execSession = null;
      call(context, "/container/exec-close", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ session })
      }).catch(() => {});
    }
    if (execDialog.open)
      execDialog.close();
  }
  async function pull() {
    const reference = pullReference.value.trim();
    if (!reference) {
      setStatus("An image reference is required.", true);
      return;
    }
    try {
      setStatus(`Pulling ${reference}…`);
      const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}/images/pull?reference=${encodeURIComponent(reference)}`, { headers: { accept: "application/octet-stream" } });
      if (!response.ok)
        throw new Error(await response.text());
      if (response.body) {
        const reader = response.body.getReader();
        const decoder = new TextDecoder;
        for (;; ) {
          const { done, value } = await reader.read();
          if (done)
            break;
          const text = decoder.decode(value, { stream: true });
          if (text.trim())
            setStatus(text.trim().slice(0, 240));
        }
      }
      setStatus(`${reference} pulled.`);
      await loadImages();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function createVolume() {
    const name = volumeName.value.trim();
    if (!name) {
      setStatus("A volume name is required.", true);
      return;
    }
    try {
      await call(context, "/volumes/create", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name })
      });
      volumeName.value = "";
      await loadVolumes();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  box.querySelector("[data-refresh]")?.addEventListener("click", () => {
    loadContainers();
  });
  createButton.addEventListener("click", () => {
    context.openModal?.({ tag: "containers-create-modal" });
  });
  for (const button of box.querySelectorAll(".cn-tab")) {
    button.addEventListener("click", () => switchTab(button.dataset.tab));
  }
  box.querySelector("[data-pull]")?.addEventListener("click", () => void pull());
  box.querySelector("[data-volume-create]")?.addEventListener("click", () => void createVolume());
  box.querySelector("[data-confirm-cancel]")?.addEventListener("click", () => {
    pending = null;
    confirmDialog.close();
  });
  box.querySelector("[data-confirm-run]")?.addEventListener("click", () => {
    const run = pending;
    pending = null;
    if (!run)
      return;
    confirmDialog.close();
    run().catch((error) => setStatus(String(error), true));
  });
  box.querySelector("[data-exec-send]")?.addEventListener("click", () => void sendExec());
  execInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      sendExec();
    }
  });
  box.querySelector("[data-exec-close]")?.addEventListener("click", () => closeExec());
  execDialog.addEventListener("close", () => {
    if (execSession) {
      const session = execSession;
      execSession = null;
      call(context, "/container/exec-close", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ session })
      }).catch(() => {});
    }
    exec?.abort();
    exec = null;
  });
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh")
      loadContainers();
  });
  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    if (next)
      loadContainers();
  });
  const initial = context.params;
  loadContainers().then(() => {
    if (initial?.refresh)
      loadContainers();
  });
  const poll = window.setInterval(() => {
    if (visible && !disposed && tab === "containers")
      loadContainers();
  }, 5000);
  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    stopDetailStreams();
    window.clearInterval(poll);
    root.replaceChildren();
  };
}
export {
  mount
};
