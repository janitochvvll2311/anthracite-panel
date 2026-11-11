// src/api.ts
async function fetchHostInfo(context) {
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}/info`, { headers: { accept: "application/json" } });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(text || `HTTP ${response.status}`);
  }
  return JSON.parse(text);
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
async function watchHostInfo(context, signal, intervalMs, onInfo) {
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}/watch?interval-ms=${intervalMs}`, { headers: { accept: "text/event-stream" }, signal });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }
  if (!response.body)
    throw new Error("the host watch has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder;
  const feed = createSseParser((data) => {
    if (signal.aborted)
      return;
    try {
      onInfo(JSON.parse(data));
    } catch {}
  });
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
      feed(decoder.decode(value, { stream: true }));
    }
    feed(decoder.decode());
  } finally {
    signal.removeEventListener("abort", cancel);
  }
}

// src/format.ts
function formatBytes(value) {
  const size = Number(value) || 0;
  if (size < 1024)
    return `${size} B`;
  if (size < 1024 ** 2)
    return `${(size / 1024).toFixed(1)} KiB`;
  if (size < 1024 ** 3)
    return `${(size / 1024 ** 2).toFixed(1)} MiB`;
  if (size < 1024 ** 4)
    return `${(size / 1024 ** 3).toFixed(1)} GiB`;
  return `${(size / 1024 ** 4).toFixed(2)} TiB`;
}
function formatDuration(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  const days = Math.floor(total / 86400);
  const hours = Math.floor(total % 86400 / 3600);
  const minutes = Math.floor(total % 3600 / 60);
  if (days)
    return `${days}d ${hours}h ${minutes}m`;
  if (hours)
    return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}
function percent(used, total) {
  const value = Number(total) > 0 ? Number(used) / Number(total) * 100 : 0;
  return Math.max(0, Math.min(100, value));
}

// src/dom.ts
function span(text, className) {
  const node = document.createElement("span");
  if (className)
    node.className = className;
  node.textContent = text;
  return node;
}
function rows(target, pairs) {
  target.replaceChildren();
  for (const [label, value] of pairs) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    target.append(term, detail);
  }
}
function meter(used, total, warm = false) {
  const wrap = document.createElement("div");
  wrap.className = warm ? "home-bar warm" : "home-bar";
  const fill = document.createElement("span");
  fill.style.width = `${percent(used, total)}%`;
  wrap.append(fill);
  return wrap;
}
function empty(text) {
  const node = document.createElement("p");
  node.className = "home-empty";
  node.textContent = text;
  return node;
}
function query(root, selector) {
  return root.querySelector(selector);
}

// src/render.ts
function renderHost(root, info) {
  renderSystem(root, info);
  renderCpu(root, info.cpu);
  renderMemory(root, info.memory);
  renderDisks(root, info.disks);
  renderNetworks(root, info.networks);
}
function renderSystem(root, info) {
  const target = query(root, "[data-system]");
  if (!target)
    return;
  rows(target, [
    ["Hostname", info.hostname],
    ["OS", info.os],
    ["Kernel", info.kernel],
    ["Architecture", info.arch],
    ["Uptime", formatDuration(info["uptime-seconds"])]
  ]);
}
function renderCpu(root, cpu) {
  const target = query(root, "[data-cpu]");
  if (!target)
    return;
  target.replaceChildren();
  const details = document.createElement("dl");
  rows(details, [
    ["Model", cpu.model],
    ["Cores", String(cpu.cores)],
    [
      "Load",
      `${Number(cpu["load-1"]).toFixed(2)} / ${Number(cpu["load-5"]).toFixed(2)} / ${Number(cpu["load-15"]).toFixed(2)}`
    ]
  ]);
  const label = document.createElement("div");
  label.className = "home-bar-label";
  label.append(span("Usage"), span(`${Number(cpu["usage-percent"]).toFixed(1)}%`));
  target.append(details, label, meter(cpu["usage-percent"], 100, true));
}
function renderMemory(root, memory) {
  const target = query(root, "[data-memory]");
  if (!target)
    return;
  target.replaceChildren();
  const label = document.createElement("div");
  label.className = "home-bar-label";
  label.append(span(`${formatBytes(memory.used)} / ${formatBytes(memory.total)}`), span(`${percent(memory.used, memory.total).toFixed(0)}%`));
  const details = document.createElement("dl");
  rows(details, [
    ["Available", formatBytes(memory.available)],
    [
      "Swap",
      memory["swap-total"] > 0 ? `${formatBytes(memory["swap-used"])} / ${formatBytes(memory["swap-total"])}` : "not configured"
    ]
  ]);
  target.append(label, meter(memory.used, memory.total), details);
}
function renderDisks(root, disks) {
  const target = query(root, "[data-disks]");
  if (!target)
    return;
  target.replaceChildren();
  if (disks.length === 0) {
    target.append(empty("No disks detected."));
    return;
  }
  for (const disk of disks) {
    const row = document.createElement("div");
    row.className = "home-disk";
    const head = document.createElement("div");
    head.className = "home-disk-head";
    head.append(span(disk.mount, "home-disk-name"), span(`${disk["file-system"]} · ${disk.name}`, "home-disk-meta"));
    const label = document.createElement("div");
    label.className = "home-bar-label";
    label.append(span(`${formatBytes(disk.used)} of ${formatBytes(disk.total)}`), span(`${formatBytes(disk.available)} free`));
    row.append(head, label, meter(disk.used, disk.total));
    target.append(row);
  }
}
function renderNetworks(root, networks) {
  const target = query(root, "[data-network]");
  if (!target)
    return;
  target.replaceChildren();
  if (networks.length === 0) {
    target.append(empty("No network interfaces detected."));
    return;
  }
  for (const network of networks) {
    const row = document.createElement("div");
    row.className = "home-net";
    const head = document.createElement("div");
    head.className = "home-net-head";
    const up = ["up", "unknown"].includes(String(network.state).toLowerCase());
    head.append(span(network.name, "home-net-name"), span(network.state || "unknown", up ? "home-net-state" : "home-net-state down"), span(network.mac ?? "no MAC", "home-net-meta"));
    const addresses = document.createElement("div");
    addresses.className = "home-net-addr";
    addresses.textContent = network.addresses.length > 0 ? network.addresses.join(" · ") : "No addresses";
    const io = document.createElement("div");
    io.className = "home-net-io";
    io.textContent = `↓ ${formatBytes(network.received)} / ${network["packets-received"]} pkts · ` + `↑ ${formatBytes(network.transmitted)} / ${network["packets-transmitted"]} pkts · ` + `${network.errors} errors`;
    row.append(head, addresses, io);
    target.append(row);
  }
}

// src/styles.ts
var styles = `
  .home { display: flex; flex-direction: column; gap: 14px; max-width: 1100px; }
  .home-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  .home h3 { margin: 0; font-size: 20px; font-weight: 600; letter-spacing: -.01em; }
  .home-sub { margin: 2px 0 0; color: #8f98ab; font-size: 12.5px; }
  .home-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .home-actions button, .home-refresh {
    padding: 7px 11px; border-radius: 7px; border: 1px solid #2a3142; cursor: pointer;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px;
  }
  .home-actions button:hover, .home-refresh:hover { border-color: #3a4560; color: #fff; }
  .home-refresh:disabled { opacity: .5; cursor: default; }
  .home-interval { display: flex; align-items: center; gap: 6px; color: #8f98ab; font-size: 12px; }
  .home-interval select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .home-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .home-card {
    border: 1px solid #202632; border-radius: 10px; background: #0f1219;
    padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; min-width: 0;
  }
  .home-card.wide { grid-column: 1 / -1; }
  .home-card h4 {
    margin: 0; color: #8f98ab; font-size: 11px; font-weight: 600;
    text-transform: uppercase; letter-spacing: .08em;
  }
  .home dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 6px 16px; margin: 0; font-size: 12.5px; }
  .home dt { color: #667087; }
  .home dd { margin: 0; color: #c8d0df; overflow-wrap: anywhere; }
  .home-bar { height: 6px; border-radius: 3px; background: #1b2130; overflow: hidden; display: flex; }
  .home-bar span { display: block; height: 100%; background: linear-gradient(90deg, #4361a8, #7aa2ff); }
  .home-bar.warm span { background: linear-gradient(90deg, #9a6a3a, #f7c948); }
  .home-bar-label { display: flex; justify-content: space-between; gap: 8px; font-size: 11px; color: #8f98ab; }
  .home-disk, .home-net {
    display: flex; flex-direction: column; gap: 5px; padding: 9px 0;
    border-top: 1px solid #171c26;
  }
  .home-disk:first-child, .home-net:first-child { border-top: 0; padding-top: 0; }
  .home-disk:last-child, .home-net:last-child { padding-bottom: 0; }
  .home-disk-head, .home-net-head { display: flex; justify-content: space-between; gap: 10px; align-items: baseline; }
  .home-disk-name, .home-net-name { color: #e8ebf2; font-weight: 600; font-size: 12.5px; }
  .home-disk-meta, .home-net-meta { color: #667087; font-size: 11px; }
  .home-net-state { text-transform: uppercase; font-size: 10px; letter-spacing: .06em; color: #9fe6cb; }
  .home-net-state.down { color: #ffb3ba; }
  .home-net-addr { font: 11px/1.5 ui-monospace, monospace; color: #b9c3d6; overflow-wrap: anywhere; }
  .home-net-io { color: #8f98ab; font-size: 11px; }
  .home-empty { color: #667087; font-size: 12px; }
  .home-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .home-status.bad { color: #ffb3ba; }
  @media (max-width: 720px) {
    .home-grid { grid-template-columns: 1fr; }
    .home-card.wide { grid-column: auto; }
  }
`;

// src/main.ts
var TEMPLATE = `
  <header class="home-head">
    <div>
      <h3>Home</h3>
      <p class="home-sub" data-subtitle>Reading host information…</p>
    </div>
    <div class="home-actions">
      <label class="home-interval">Interval
        <select data-interval aria-label="Watch interval">
          <option value="0">Manual (0s)</option>
          <option value="1000">1s</option>
          <option value="2000" selected>2s</option>
          <option value="5000">5s</option>
          <option value="10000">10s</option>
        </select>
      </label>
      <button class="home-refresh" type="button" data-refresh>Refresh</button>
    </div>
  </header>
  <div class="home-grid">
    <section class="home-card"><h4>System</h4><dl data-system></dl></section>
    <section class="home-card"><h4>CPU</h4><div data-cpu></div></section>
    <section class="home-card"><h4>Memory</h4><div data-memory></div></section>
    <section class="home-card wide"><h4>Disks</h4><div data-disks></div></section>
    <section class="home-card wide"><h4>Network</h4><div data-network></div></section>
  </div>
  <p class="home-status" data-status role="status"></p>
`;
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = "home";
  box.innerHTML = TEMPLATE;
  root.append(box);
  const subtitle = query(box, "[data-subtitle]");
  const status = query(box, "[data-status]");
  const refresh = query(box, "[data-refresh]");
  const interval = query(box, "[data-interval]");
  if (!subtitle || !status || !refresh || !interval) {
    return () => root.replaceChildren();
  }
  let disposed = false;
  let loading = false;
  let intervalMs = 2000;
  let controller;
  function setStatus(message, bad = false) {
    status.textContent = message;
    status.className = bad ? "home-status bad" : "home-status";
  }
  async function load() {
    if (loading || disposed)
      return;
    loading = true;
    refresh.disabled = true;
    setStatus("Reading host information…");
    try {
      const info = await fetchHostInfo(context);
      if (disposed)
        return;
      subtitle.textContent = `${info.hostname} · ${info.os}`;
      renderHost(box, info);
      setStatus(`Updated ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      if (!disposed) {
        setStatus(`Could not read host information: ${error instanceof Error ? error.message : String(error)}`, true);
      }
    } finally {
      loading = false;
      if (!disposed)
        refresh.disabled = false;
    }
  }
  async function watch() {
    if (disposed || intervalMs === 0)
      return;
    const abort = new AbortController;
    controller = abort;
    setStatus("Watching host information…");
    try {
      await watchHostInfo(context, abort.signal, intervalMs, (info) => {
        if (disposed)
          return;
        subtitle.textContent = `${info.hostname} · ${info.os}`;
        renderHost(box, info);
        setStatus(`Live · ${new Date().toLocaleTimeString()}`);
      });
      if (!disposed && !abort.signal.aborted) {
        setStatus("The host watch ended.", true);
      }
    } catch (error) {
      if (disposed || abort.signal.aborted)
        return;
      setStatus(`Could not watch host information: ${error instanceof Error ? error.message : String(error)}`, true);
      load();
    } finally {
      if (controller === abort)
        controller = undefined;
    }
  }
  function restart() {
    controller?.abort();
    controller = undefined;
    if (intervalMs > 0)
      watch();
    else
      load();
  }
  refresh.addEventListener("click", restart);
  interval.addEventListener("change", () => {
    intervalMs = Number(interval.value) || 0;
    restart();
  });
  intervalMs > 0 ? watch() : load();
  return () => {
    disposed = true;
    controller?.abort();
    controller = undefined;
    root.replaceChildren();
  };
}
export {
  mount
};
