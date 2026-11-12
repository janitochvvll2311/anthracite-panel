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
async function watchNetwork(context, signal, intervalMs, onSnapshot) {
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}/watch?interval-ms=${intervalMs}`, { headers: { accept: "text/event-stream" }, signal });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }
  if (!response.body)
    throw new Error("the network watch has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder;
  const feed = createSseParser((data) => {
    if (signal.aborted)
      return;
    try {
      onSnapshot(JSON.parse(data));
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
function count(bytes) {
  if (bytes >= 1024 ** 3)
    return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
  if (bytes >= 1024 ** 2)
    return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  if (bytes >= 1024)
    return `${(bytes / 1024).toFixed(1)} KiB`;
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
  .net { display: flex; flex-direction: column; gap: 14px; min-height: 400px; }
  .net-bar { display: flex; align-items: center; gap: 10px; }
  .net-bar .grow { flex: 1; }
  .net-interval { display: flex; align-items: center; gap: 6px; color: #8f98ab; font-size: 12px; }
  .net-interval select {
    padding: 5px 8px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .net-bar button, .net-add button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .net-bar button:hover, .net-add button:hover { border-color: #3a4560; color: #fff; }
  .net-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 10px; }
  .net-card {
    border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 11px 12px;
    display: grid; gap: 7px;
  }
  .net-card-head { display: flex; align-items: center; gap: 8px; }
  .net-name { font-weight: 600; color: #e8ebf2; }
  .net-state { margin-left: auto; font-size: 10px; text-transform: uppercase; letter-spacing: .08em; }
  .net-state.up { color: #52d1a4; }
  .net-state.down { color: #667087; }
  .net-toggle { padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24; color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer; }
  .net-toggle:hover { border-color: #3a4560; color: #fff; }
  .net-meta { color: #667087; font: 11px ui-monospace, monospace; display: flex; gap: 12px; flex-wrap: wrap; }
  .net-addrs { display: flex; gap: 5px; flex-wrap: wrap; }
  .net-chip {
    padding: 2px 7px; border-radius: 999px; border: 1px solid #2a3142; color: #b9c3d6;
    font: 11px ui-monospace, monospace;
  }
  .net-chip.ipv6 { border-color: #9d7bff44; color: #cbb8ff; }
  .net-counters { display: flex; gap: 14px; color: #8f98ab; font-size: 11px; }
  .net-counters b { color: #c8d0df; font-weight: 500; }
  .net-routes { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .net-route {
    display: grid; grid-template-columns: 60px minmax(150px, 1fr) minmax(120px, 1fr) 100px 70px 30px;
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center;
  }
  .net-route:last-child { border-bottom: 0; }
  .net-route.head {
    position: sticky; top: 0; background: #11151e; color: #667087; text-transform: uppercase;
    letter-spacing: .06em; font-size: 10px;
  }
  .net-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .net-remove {
    width: 22px; height: 22px; border: 0; border-radius: 5px; background: transparent;
    color: #667087; cursor: pointer; font-size: 13px; line-height: 1;
  }
  .net-remove:hover { background: #2b3344; color: #ff7d8a; }
  .net-add { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .net-add input, .net-add select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .net-add input[data-destination] { flex: 1; min-width: 120px; }
  .net-add input[data-gateway] { flex: 1; min-width: 110px; }
  .net-add input[data-metric] { width: 70px; }
  .net-add .hint { color: #667087; font-size: 11px; }
  .net-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .net-status.bad { color: #ffb3ba; }

  @media (max-width: 640px) {
    .net-bar { flex-wrap: wrap; }
    .net-bar .net-mono { flex: 1 0 100%; }
    .net-bar .grow { display: none; }
    .net-interval { margin-left: auto; }
    .net-routes { overflow: visible; border: 0; background: transparent; }
    .net-route.head { display: none; }
    .net-route {
      display: flex; flex-wrap: wrap; gap: 4px 12px; align-items: center;
      position: relative; padding: 10px 40px 10px 12px; margin-bottom: 8px;
      border: 1px solid #202632; border-radius: 9px; background: #0d1016;
    }
    .net-route:last-child { margin-bottom: 0; }
    .net-route > span { display: inline-flex; gap: 4px; align-items: baseline; }
    .net-route > span[data-label]::before {
      content: attr(data-label); color: #667087; font-size: 9px;
      text-transform: uppercase; letter-spacing: .06em;
    }
    .net-route > :nth-child(2) { order: -1; flex: 1 0 100%; color: #e8ebf2; font-weight: 600; }
    .net-route > :nth-child(2)::before { content: none; }
    .net-route > :nth-child(6) { position: absolute; top: 6px; right: 6px; margin: 0; }
  }

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

// src/main.ts
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = "net";
  box.innerHTML = `
    <div class="net-bar">
      <span class="net-mono">interfaces and routes</span>
      <span class="grow"></span>
      <label class="net-interval">Interval
        <select data-interval aria-label="Watch interval">
          <option value="0">Manual (0s)</option>
          <option value="1000">1s</option>
          <option value="2000" selected>2s</option>
          <option value="5000">5s</option>
          <option value="10000">10s</option>
        </select>
      </label>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="net-grid"></div>
    <div class="net-routes" role="table" aria-label="Routes"></div>
    <p class="net-status" role="status"></p>
  `;
  root.append(box);
  const grid = box.querySelector(".net-grid");
  const routes = box.querySelector(".net-routes");
  const status = box.querySelector(".net-status");
  const interval = box.querySelector("[data-interval]");
  let intervalMs = 2000;
  let controller;
  let visible = true;
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "net-status bad" : "net-status";
  };
  function render(interfaces, table) {
    const active = document.activeElement;
    const focusKey = active?.dataset.key;
    const routesScroll = routes.scrollTop;
    grid.replaceChildren();
    for (const iface of interfaces) {
      const card = document.createElement("section");
      card.className = "net-card";
      const head = document.createElement("div");
      head.className = "net-card-head";
      const toggle = document.createElement("button");
      toggle.type = "button";
      toggle.className = "net-toggle";
      toggle.dataset.key = `iface:${iface.name}`;
      toggle.textContent = iface.up ? "Down" : "Up";
      toggle.title = `${iface.up ? "Bring down" : "Bring up"} ${iface.name}`;
      toggle.addEventListener("click", () => void setUp(iface.name, !iface.up));
      head.append(cell("net-name", iface.name), cell(`net-state ${iface.up ? "up" : "down"}`, iface.up ? "up" : "down"), toggle);
      const meta = document.createElement("div");
      meta.className = "net-meta";
      meta.append(cell("", `mac ${iface.mac}`), cell("", `mtu ${iface.mtu}`));
      const addrs = document.createElement("div");
      addrs.className = "net-addrs";
      for (const address of iface.addresses) {
        addrs.append(cell(`net-chip ${address.family}`, `${address.address}/${address.prefix}`));
      }
      const counters = document.createElement("div");
      counters.className = "net-counters";
      counters.innerHTML = `<span>rx <b>${count(iface["rx-bytes"])}</b></span><span>tx <b>${count(iface["tx-bytes"])}</b></span>`;
      card.append(head, meta, addrs, counters);
      grid.append(card);
    }
    routes.replaceChildren();
    const head = document.createElement("div");
    head.className = "net-route head";
    for (const label of ["FAMILY", "DESTINATION", "GATEWAY", "DEVICE", "METRIC", ""]) {
      head.append(cell("", label));
    }
    routes.append(head);
    for (const route of table) {
      const row = document.createElement("div");
      row.className = "net-route";
      const familyCell = cell("net-mono", route.family);
      familyCell.dataset.label = "Family";
      const destinationCell = cell("net-mono", route.destination);
      destinationCell.dataset.label = "Destination";
      const gatewayCell = cell("net-mono", route.gateway ?? "—");
      gatewayCell.dataset.label = "Gateway";
      const deviceCell = cell("", route.device);
      deviceCell.dataset.label = "Device";
      const metricCell = cell("net-mono", String(route.metric));
      metricCell.dataset.label = "Metric";
      row.append(familyCell, destinationCell, gatewayCell, deviceCell, metricCell);
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "net-remove";
      remove.dataset.key = `route:${route.destination}:${route.device}`;
      remove.title = `Remove ${route.destination} via ${route.device}`;
      remove.textContent = "×";
      remove.addEventListener("click", () => void removeRoute(route));
      const action = document.createElement("span");
      action.append(remove);
      row.append(action);
      routes.append(row);
    }
    routes.scrollTop = routesScroll;
    if (focusKey) {
      const escaped = CSS.escape(focusKey);
      (grid.querySelector(`[data-key="${escaped}"]`) ?? routes.querySelector(`[data-key="${escaped}"]`))?.focus();
    }
  }
  async function load() {
    setStatus("Reading the network…");
    try {
      const [interfaces, table] = await Promise.all([
        call(context, "/interfaces"),
        call(context, "/routes")
      ]);
      if (disposed)
        return;
      render(interfaces, table);
      setStatus(`Updated ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      if (!disposed)
        setStatus(reason(error), true);
    }
  }
  async function watch() {
    if (disposed || intervalMs === 0)
      return;
    const abort = new AbortController;
    controller = abort;
    setStatus("Watching the network…");
    try {
      await watchNetwork(context, abort.signal, intervalMs, (snapshot) => {
        if (disposed)
          return;
        render(snapshot.interfaces, snapshot.routes);
        setStatus(`Live · ${new Date().toLocaleTimeString()}`);
      });
      if (!disposed && !abort.signal.aborted) {
        setStatus("The network watch ended.", true);
      }
    } catch (error) {
      if (disposed || abort.signal.aborted)
        return;
      setStatus(`Could not watch the network: ${reason(error)}`, true);
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
  async function setUp(name, up) {
    const ok = await confirmAction({
      title: up ? `Bring up ${name}?` : `Bring down ${name}?`,
      message: up ? "The interface will start forwarding traffic again." : "Traffic on this interface will stop until it is brought back up.",
      confirmLabel: up ? "Bring up" : "Bring down",
      danger: !up
    });
    if (!ok)
      return;
    setStatus(`${up ? "Bringing up" : "Bringing down"} ${name}…`);
    try {
      await call(context, "/up", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name, up })
      });
      await load();
      setStatus(`${name} is ${up ? "up" : "down"}`);
    } catch (error) {
      if (!disposed)
        setStatus(reason(error), true);
    }
  }
  async function removeRoute(route) {
    const ok = await confirmAction({
      title: `Remove ${route.destination}?`,
      message: `Traffic routed through ${route.device} will stop using this entry.`,
      confirmLabel: "Remove route",
      danger: true
    });
    if (!ok)
      return;
    setStatus(`Removing ${route.destination}…`);
    try {
      await call(context, `/route?destination=${encodeURIComponent(route.destination)}&device=${encodeURIComponent(route.device)}`, { method: "DELETE" });
      await load();
      setStatus(`Removed ${route.destination} via ${route.device}`);
    } catch (error) {
      if (!disposed)
        setStatus(reason(error), true);
    }
  }
  interval.addEventListener("change", () => {
    intervalMs = Number(interval.value) || 0;
    restart();
  });
  box.querySelector("[data-refresh]").addEventListener("click", restart);
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh")
      load();
  });
  const offVisibility = context.onVisibility?.((next) => {
    if (next === visible)
      return;
    visible = next;
    if (!visible) {
      controller?.abort();
      controller = undefined;
    } else if (intervalMs > 0) {
      watch();
    }
  });
  intervalMs > 0 ? watch() : load();
  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    controller?.abort();
    controller = undefined;
    root.replaceChildren();
  };
}
export {
  mount
};
