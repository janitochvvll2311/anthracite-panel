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
function applyBody(config, expectedVersion) {
  return { config, "expected-version": expectedVersion };
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
async function watchLogs(context, signal, onEvent) {
  const response = await context.fetch(route(context, "/logs"), {
    headers: { accept: "text/event-stream" },
    signal
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }
  if (!response.body)
    throw new Error("the access log has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder;
  const feed = createSseParser((data) => {
    if (signal.aborted)
      return;
    try {
      onEvent(JSON.parse(data));
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

// src/styles.ts
var styles = `
  .px { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .px-field { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .px-field-name {
    display: none; font-size: 10px; letter-spacing: .04em; text-transform: uppercase;
    color: #7a8497;
  }
  .px-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .px-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .px-tabs { display: flex; gap: 4px; }
  .px-tabs button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #11151d; color: #98a2b6; font: inherit; font-size: 12px; cursor: pointer;
  }
  .px-tabs button.active { border-color: #4a5c82; background: #151d2c; color: #fff; }
  .grow { flex: 1; }
  .px button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .px button:hover { border-color: #4a5c82; color: #fff; }
  .px button.primary { border-color: #33507a; background: #16233a; color: #dbe6ff; }
  .px button.danger { border-color: #5c2b31; color: #ffb3ba; }
  .px-status { margin: 0; font-size: 12px; color: #8f98ab; min-height: 16px; }
  .px-status.bad { color: #ffb3ba; }
  .px-banner {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #5c2b31;
    background: #241214; color: #ffb3ba; font-size: 12px;
  }
  .px-note { font-size: 12px; color: #8f98ab; }
  .px-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: #8f98ab; }
  .px-section { display: flex; flex-direction: column; gap: 12px; }
  .px-card {
    border: 1px solid #232a38; border-radius: 9px; padding: 10px 12px;
    background: #0f131a; display: flex; flex-direction: column; gap: 8px;
  }
  .px-card-head { display: flex; align-items: center; gap: 8px; }
  .px-card-head strong { color: #dfe5f1; font-size: 13px; }
  .px-grid {
    display: grid; grid-template-columns: 90px 1fr 90px 90px 70px 140px;
    gap: 8px; align-items: center; font-size: 12px;
  }
  .px-grid.hosts { grid-template-columns: 90px 1fr 140px 70px 70px 70px 60px; }
  .px-grid.head { color: #7a8497; font-size: 11px; }
  .px-grid input[type="text"], .px-grid input[type="number"], .px-grid select {
    width: 100%; padding: 4px 6px; border-radius: 6px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 12px;
  }
  .px-form {
    display: flex; gap: 8px; align-items: center; flex-wrap: wrap;
    padding-top: 4px; border-top: 1px dashed #232a38;
  }
  .px-form input[type="text"], .px-form input[type="number"], .px-form select {
    padding: 4px 6px; border-radius: 6px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 12px;
  }
  .px-check { display: inline-flex; align-items: center; gap: 4px; color: #98a2b6; }
  .px-log {
    display: flex; flex-direction: column; gap: 2px; max-height: 46vh;
    overflow: auto; border: 1px solid #232a38; border-radius: 8px; background: #0d1117;
  }
  .px-event {
    display: grid; grid-template-columns: 64px 120px 60px 1fr 60px 60px 70px;
    gap: 8px; padding: 4px 8px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11.5px; color: #b9c2d4; border-bottom: 1px solid #141a24;
  }
  .px-event .bad { color: #ffb3ba; }
  .px-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid #2a3142;
    font-size: 11px; color: #98a2b6;
  }
  .px-badge.ok { border-color: #2f5a3f; color: #93d9ab; }
  .px-badge.bad { border-color: #5c2b31; color: #ffb3ba; }
  .px-empty { padding: 10px; color: #7a8497; font-size: 12px; }
  .px-card { min-width: 0; }

  /* The panel's explicit developer modes reach the frame as
     html[data-viewport], so the view adapts even when the iframe stays wide
     on a desktop browser. */
  html[data-viewport="tablet"] .px-grid,
  html[data-viewport="tablet"] .px-grid.hosts { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  html[data-viewport="tablet"] .px-grid.head { display: none; }
  html[data-viewport="tablet"] .px-field-name { display: block; }
  html[data-viewport="tablet"] .px-card-head { flex-wrap: wrap; }
  html[data-viewport="tablet"] .px-event { grid-template-columns: 60px 1fr 60px 60px; }
  html[data-viewport="tablet"] .px-event > :nth-child(3),
  html[data-viewport="tablet"] .px-event > :nth-child(6),
  html[data-viewport="tablet"] .px-event > :nth-child(7) { display: none; }

  html[data-viewport="mobile"] .px-grid,
  html[data-viewport="mobile"] .px-grid.hosts { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .px-grid.head { display: none; }
  html[data-viewport="mobile"] .px-field-name { display: block; }
  html[data-viewport="mobile"] .px-card-head { flex-wrap: wrap; }
  html[data-viewport="mobile"] .px-event { grid-template-columns: 58px 1fr 52px; }
  html[data-viewport="mobile"] .px-event > :nth-child(2) { display: none; }
  html[data-viewport="mobile"] .px-form input[type="text"],
  html[data-viewport="mobile"] .px-form input[type="number"],
  html[data-viewport="mobile"] .px-form select { flex: 1 1 100%; }
  html[data-viewport="mobile"] .px-form button { width: 100%; }
  html[data-viewport="mobile"] .px-actions { width: 100%; }

  /* The view is responsive to the frame width, which is what the panel's
     desktop/tablet/mobile modes change. Containers, not the page width: the
     app lives in an iframe. */
  @container (max-width: 980px) {
    .px-grid, .px-grid.hosts { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .px-grid.head { display: none; }
    .px-field-name { display: block; }
    .px-card-head { flex-wrap: wrap; }
    .px-event { grid-template-columns: 60px 1fr 60px 60px; }
    .px-event > :nth-child(3),
    .px-event > :nth-child(6),
    .px-event > :nth-child(7) { display: none; }
  }

  @container (max-width: 640px) {
    .px-grid, .px-grid.hosts { grid-template-columns: minmax(0, 1fr); }
    .px-event { grid-template-columns: 58px 1fr 52px; }
    .px-event > :nth-child(2) { display: none; }
    .px-form input[type="text"],
    .px-form input[type="number"],
    .px-form select { flex: 1 1 100%; }
    .px-form button { width: 100%; }
    .px-actions { width: 100%; }
  }
`;

// src/main.ts
var pendingHost = null;
function onParams(params) {
  if (!params || typeof params !== "object")
    return;
  const host = params.addHost;
  if (host)
    pendingHost?.(host);
}
function clone(value) {
  return JSON.parse(JSON.stringify(value));
}
function escape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function stamp(seconds) {
  return seconds ? new Date(seconds * 1000).toLocaleTimeString() : "—";
}
function option(value, label, selected) {
  return `<option value="${escape(value)}"${selected ? " selected" : ""}>${escape(label)}</option>`;
}
function uniqueId(existing, base) {
  if (!existing.includes(base))
    return base;
  let index = 2;
  while (existing.includes(`${base}-${index}`))
    index += 1;
  return `${base}-${index}`;
}
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = "px";
  box.dataset.testid = "proxy-view";
  box.innerHTML = `
    <div class="px-bar">
      <div class="px-tabs" role="tablist">
        <button type="button" role="tab" data-section="listeners" data-testid="proxy-tab-listeners">Listeners</button>
        <button type="button" role="tab" data-section="hosts" data-testid="proxy-tab-hosts">Hosts &amp; routes</button>
        <button type="button" role="tab" data-section="upstreams" data-testid="proxy-tab-upstreams">Upstreams</button>
        <button type="button" role="tab" data-section="log" data-testid="proxy-tab-log">Access log</button>
      </div>
      <span class="grow"></span>
      <span class="px-badge" data-running>—</span>
      <button type="button" data-reload-certs data-testid="proxy-reload-certs" title="Re-read the published certificates">Reload certs</button>
      <button type="button" data-refresh data-testid="proxy-refresh">Refresh</button>
      <button type="button" class="primary" data-apply data-testid="proxy-apply">Apply</button>
    </div>
    <div class="px-banner" data-banner data-testid="proxy-banner" hidden></div>
    <div class="px-section" data-body data-testid="proxy-body"></div>
    <p class="px-status" role="status" data-status data-testid="proxy-status"></p>
  `;
  root.append(box);
  const body = box.querySelector("[data-body]");
  const banner = box.querySelector("[data-banner]");
  const statusLine = box.querySelector("[data-status]");
  const running = box.querySelector("[data-running]");
  let config = null;
  let draft = null;
  let certificates = [];
  let status = null;
  let section = "listeners";
  let events = [];
  let controller = null;
  let disposed = false;
  const setStatus = (message, bad = false) => {
    statusLine.textContent = message;
    statusLine.className = bad ? "px-status bad" : "px-status";
  };
  const showStatus = () => {
    if (!status) {
      running.textContent = "—";
      running.className = "px-badge";
      banner.hidden = true;
      return;
    }
    running.textContent = status.running ? "running" : "stopped";
    running.className = status.running ? "px-badge ok" : "px-badge bad";
    running.title = status.listeners.join(", ");
    banner.hidden = !status["last-error"];
    banner.textContent = status["last-error"] ?? "";
  };
  const requireDraft = () => {
    if (!draft) {
      setStatus("The document has not loaded yet.", true);
      return null;
    }
    return draft;
  };
  async function refreshStatus() {
    try {
      status = await call(context, "/status");
      if (!disposed)
        showStatus();
    } catch {}
  }
  async function load() {
    setStatus("Reading the proxy document…");
    try {
      const [next, state, ids] = await Promise.all([
        call(context, "/config"),
        call(context, "/status"),
        call(context, "/certificates")
      ]);
      if (disposed)
        return;
      config = next;
      draft = clone(next);
      status = state;
      certificates = Array.isArray(ids) ? ids : [];
      showStatus();
      render();
      setStatus(`Loaded v${next.version}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function apply() {
    const document2 = requireDraft();
    if (!document2 || !config)
      return;
    setStatus("Validating and applying the whole document…");
    try {
      const next = await call(context, "/config", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(applyBody(document2, config.version))
      });
      if (disposed)
        return;
      config = next;
      draft = clone(next);
      await refreshStatus();
      if (disposed)
        return;
      render();
      setStatus(`Applied v${next.version}`);
    } catch (error) {
      if (disposed)
        return;
      setStatus(String(error), true);
      await refreshStatus();
    }
  }
  async function reloadCertificates() {
    setStatus("Re-reading the published certificates…");
    try {
      status = await call(context, "/certificates/reload", {
        method: "POST"
      });
      if (disposed)
        return;
      showStatus();
      setStatus("Certificates reloaded");
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function testUpstream(id) {
    setStatus(`Testing ${id}…`);
    try {
      const result = await call(context, "/upstreams/test", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id })
      });
      if (disposed)
        return;
      const state = result.ok ? "ok" : "failed";
      setStatus(`${id}: ${state} · ${result.status ?? "no status"} · ${result["latency-ms"]} ms${result.error ? ` · ${result.error}` : ""}`, !result.ok);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  function stopLogs() {
    controller?.abort();
    controller = null;
  }
  function startLogs() {
    stopLogs();
    const abort = new AbortController;
    controller = abort;
    setStatus("Following the access log…");
    watchLogs(context, abort.signal, (event) => {
      if (disposed || section !== "log")
        return;
      events.push(event);
      if (events.length > 200)
        events.splice(0, events.length - 200);
      appendEvent(event);
    }).then(() => {
      if (!disposed && !abort.signal.aborted) {
        setStatus("The access log ended.", true);
      }
    }).catch((error) => {
      if (!disposed && !abort.signal.aborted)
        setStatus(String(error), true);
    });
  }
  function appendEvent(event) {
    const list = body.querySelector("[data-log]");
    if (!list)
      return;
    list.insertAdjacentHTML("beforeend", eventRow(event));
    list.scrollTop = list.scrollHeight;
    const count = body.querySelector("[data-log-count]");
    if (count)
      count.textContent = String(events.length);
  }
  function eventRow(event) {
    const bad = event.status >= 500 ? " bad" : "";
    return `<div class="px-event">
      <span>${stamp(event.at)}</span>
      <span>${escape(event.host)}</span>
      <span>${escape(event.method)}</span>
      <span>${escape(event.path)}</span>
      <span class="${bad.trim()}">${event.status}</span>
      <span>${event.bytes}</span>
      <span>${escape(event.upstream || "—")}</span>
    </div>`;
  }
  function renderTabs() {
    box.querySelectorAll("[data-section]").forEach((button) => {
      button.classList.toggle("active", button.dataset.section === section);
    });
  }
  function render() {
    renderTabs();
    if (!draft) {
      body.innerHTML = `<div class="px-empty">Reading the document…</div>`;
      return;
    }
    const leavingLog = section !== "log";
    if (leavingLog)
      stopLogs();
    if (section === "listeners")
      renderListeners();
    else if (section === "upstreams")
      renderUpstreams();
    else if (section === "hosts")
      renderHosts();
    else
      renderLogs();
  }
  function renderListeners() {
    const document2 = requireDraft();
    if (!document2)
      return;
    const rows = document2.listeners.map((listener, index) => {
      const hosts = document2.hosts.map((host) => option(host.id, `${host.id} (${host.names.join(", ")})`, listener["default-host"] === host.id)).join("");
      return `<div class="px-grid hosts" data-testid="proxy-listener" data-listener-id="${escape(listener.id)}">
          <span class="px-field"><span class="px-field-name">Id</span><span class="px-mono">${escape(listener.id)}</span></span>
          <span class="px-field"><span class="px-field-name">Bind</span><span class="px-mono">${escape(listener.bind)}</span></span>
          <span class="px-field"><span class="px-field-name">Kind</span><span>${listener.tls ? "TLS" : "HTTP"}</span></span>
          <span class="px-field"><span class="px-field-name">Default host</span><select data-default-host="${index}">
            ${option("", "(none)", listener["default-host"] === null)}
            ${hosts}
          </select></span>
          <span class="px-field"><span class="px-field-name">Trust</span><label class="px-check"><input type="checkbox" data-forwarded="${index}"${listener.forwarded ? " checked" : ""} /> forwarded</label></span>
          <span class="px-field"></span>
        </div>`;
    }).join("");
    body.innerHTML = `<div class="px-note">The binds are frozen in the manifest; the document maps hosts onto them and cannot open another port.</div>
      <div class="px-grid hosts head"><span>Id</span><span>Bind</span><span>Kind</span><span>Default host</span><span>Trust</span><span></span></div>
      ${rows || `<div class="px-empty">No listener in the document.</div>`}`;
    body.querySelectorAll("[data-default-host]").forEach((select) => {
      select.addEventListener("change", () => {
        const listener = draft?.listeners[Number(select.dataset.defaultHost)];
        if (!listener)
          return;
        listener["default-host"] = select.value || null;
      });
    });
    body.querySelectorAll("[data-forwarded]").forEach((input) => {
      input.addEventListener("change", () => {
        const listener = draft?.listeners[Number(input.dataset.forwarded)];
        if (!listener)
          return;
        listener.forwarded = input.checked;
      });
    });
  }
  function renderUpstreams() {
    const document2 = requireDraft();
    if (!document2)
      return;
    const rows = document2.upstreams.map((upstream, index) => `<div class="px-grid" data-testid="proxy-upstream" data-upstream-id="${escape(upstream.id)}">
          <span class="px-field"><span class="px-field-name">Id</span><span class="px-mono">${escape(upstream.id)}</span></span>
          <span class="px-field"><span class="px-field-name">URL</span><input type="text" data-upstream-url="${index}" value="${escape(upstream.url)}" /></span>
          <span class="px-field"><span class="px-field-name">Connect ms</span><input type="number" min="1" max="300000" data-upstream-connect="${index}" value="${upstream["connect-timeout-ms"]}" /></span>
          <span class="px-field"><span class="px-field-name">Read ms</span><input type="number" min="1" max="300000" data-upstream-read="${index}" value="${upstream["read-timeout-ms"]}" /></span>
          <span class="px-field"><span class="px-field-name">TLS</span><label class="px-check"><input type="checkbox" data-upstream-verify="${index}"${upstream["tls-verify"] ? " checked" : ""} /> verify</label></span>
          <span class="px-field"><span class="px-field-name">Actions</span><span class="px-actions">
            <button type="button" data-test-upstream="${escape(upstream.id)}">Test</button>
            <button type="button" class="danger" data-remove-upstream="${index}">Remove</button>
          </span></span>
        </div>`).join("");
    body.innerHTML = `<div class="px-grid head"><span>Id</span><span>URL</span><span>Connect ms</span><span>Read ms</span><span>TLS</span><span></span></div>
      ${rows || `<div class="px-empty">No upstream in the document.</div>`}
      <form class="px-form" data-add-upstream>
        <input type="text" name="id" placeholder="upstream id" required />
        <input type="text" name="url" placeholder="http://127.0.0.1:3000" required />
        <button type="submit">Add upstream</button>
      </form>`;
    body.querySelectorAll("[data-upstream-url]").forEach((input) => {
      input.addEventListener("input", () => {
        const upstream = draft?.upstreams[Number(input.dataset.upstreamUrl)];
        if (upstream)
          upstream.url = input.value;
      });
    });
    body.querySelectorAll("[data-upstream-connect]").forEach((input) => {
      input.addEventListener("input", () => {
        const upstream = draft?.upstreams[Number(input.dataset.upstreamConnect)];
        if (upstream)
          upstream["connect-timeout-ms"] = Number(input.value) || 0;
      });
    });
    body.querySelectorAll("[data-upstream-read]").forEach((input) => {
      input.addEventListener("input", () => {
        const upstream = draft?.upstreams[Number(input.dataset.upstreamRead)];
        if (upstream)
          upstream["read-timeout-ms"] = Number(input.value) || 0;
      });
    });
    body.querySelectorAll("[data-upstream-verify]").forEach((input) => {
      input.addEventListener("change", () => {
        const upstream = draft?.upstreams[Number(input.dataset.upstreamVerify)];
        if (upstream)
          upstream["tls-verify"] = input.checked;
      });
    });
    body.querySelectorAll("[data-remove-upstream]").forEach((button) => {
      button.addEventListener("click", () => {
        draft?.upstreams.splice(Number(button.dataset.removeUpstream), 1);
        render();
      });
    });
    body.querySelectorAll("[data-test-upstream]").forEach((button) => {
      button.addEventListener("click", () => {
        testUpstream(button.dataset.testUpstream ?? "");
      });
    });
    const form = body.querySelector("[data-add-upstream]");
    form?.addEventListener("submit", (event) => {
      event.preventDefault();
      const data = new FormData(form);
      const id = String(data.get("id") ?? "").trim();
      const url = String(data.get("url") ?? "").trim();
      if (!document2 || !id || !url)
        return;
      document2.upstreams.push({
        id,
        url,
        "connect-timeout-ms": 2000,
        "read-timeout-ms": 5000,
        "tls-verify": true
      });
      render();
    });
  }
  function renderHosts() {
    const document2 = requireDraft();
    if (!document2)
      return;
    const cards = document2.hosts.map((host, index) => {
      const options = [
        option("", "(none)", host.certificate === null),
        ...certificates.map((id) => option(id, id, host.certificate === id))
      ].join("");
      const routes = host.routes.map((route, routeIndex) => `<div class="px-grid hosts" data-testid="proxy-route" data-route-id="${escape(route.id)}">
              <span class="px-field"><span class="px-field-name">Match</span><span class="px-mono">${escape(route.match)}</span></span>
              <span class="px-field"><span class="px-field-name">Target</span><span class="px-mono">${escape(route.redirect ? `→ ${route.redirect}` : route.upstream ?? "—")}</span></span>
              <span class="px-field"><span class="px-field-name">Flags</span><span>${route["strip-prefix"] ? "strip" : ""}${route.websocket ? " ws" : ""}</span></span>
              <span class="px-field"><span class="px-field-name">Limit</span><span>${route["body-limit"] ?? ""}</span></span>
              <span class="px-field"></span>
              <span class="px-field"><span class="px-field-name">Actions</span><button type="button" class="danger" data-remove-route="${index}:${routeIndex}">Remove</button></span>
              <span class="px-field"></span>
            </div>`).join("");
      const upstreamOptions = [
        option("", "(none)", false),
        ...document2.upstreams.map((upstream) => option(upstream.id, upstream.id, false))
      ].join("");
      return `<div class="px-card" data-testid="proxy-host" data-host-id="${escape(host.id)}">
          <div class="px-card-head">
            <strong>${escape(host.id)}</strong>
            <span class="grow"></span>
            <button type="button" class="danger" data-remove-host="${index}">Remove host</button>
          </div>
          <div class="px-grid hosts">
            <span class="px-field"><span class="px-field-name">Names</span><input type="text" data-host-names="${index}" value="${escape(host.names.join(", "))}" /></span>
            <span class="px-field"><span class="px-field-name">Certificate</span><select data-host-certificate="${index}">${options}</select></span>
            <span class="px-field"><span class="px-field-name">ACME</span><label class="px-check"><input type="checkbox" data-host-acme="${index}"${host.acme ? " checked" : ""} /> acme</label></span>
            <span class="px-field"><span class="px-field-name">HTTPS</span><label class="px-check"><input type="checkbox" data-host-redirect="${index}"${host["redirect-https"] ? " checked" : ""} /> redirect</label></span>
            <span class="px-field"><span class="px-field-name">HSTS</span><label class="px-check"><input type="checkbox" data-host-hsts="${index}"${host.hsts ? " checked" : ""} /> hsts</label></span>
            <span class="px-field"></span>
            <span class="px-field"></span>
          </div>
          <div class="px-grid hosts head"><span>Match</span><span>Target</span><span>Flags</span><span>Limit</span><span></span><span></span><span></span></div>
          ${routes || `<div class="px-empty">No route on this host.</div>`}
          <form class="px-form" data-add-route="${index}">
            <input type="text" name="match" placeholder="= /health or /" required />
            <select name="upstream">${upstreamOptions}</select>
            <input type="text" name="redirect" placeholder="optional redirect" />
            <label class="px-check"><input type="checkbox" name="strip" /> strip</label>
            <label class="px-check"><input type="checkbox" name="websocket" /> ws</label>
            <input type="number" name="limit" min="1" placeholder="body limit" />
            <button type="submit">Add route</button>
          </form>
        </div>`;
    }).join("");
    body.innerHTML = `<div class="px-bar"><span class="px-note">A host is one certificate and one route table; an unmatched name goes to the listener default.</span><span class="grow"></span><button type="button" data-add-host data-testid="proxy-add-host">Add host</button></div>
      ${cards || `<div class="px-empty">No host in the document.</div>`}`;
    body.querySelectorAll("[data-host-names]").forEach((input) => {
      input.addEventListener("input", () => {
        const host = draft?.hosts[Number(input.dataset.hostNames)];
        if (host)
          host.names = input.value.split(",").map((name) => name.trim()).filter(Boolean);
      });
    });
    body.querySelectorAll("[data-host-certificate]").forEach((select) => {
      select.addEventListener("change", () => {
        const host = draft?.hosts[Number(select.dataset.hostCertificate)];
        if (host)
          host.certificate = select.value || null;
      });
    });
    for (const [selector, key, field] of [
      ["[data-host-acme]", "hostAcme", "acme"],
      ["[data-host-redirect]", "hostRedirect", "redirect-https"],
      ["[data-host-hsts]", "hostHsts", "hsts"]
    ]) {
      body.querySelectorAll(selector).forEach((input) => {
        input.addEventListener("change", () => {
          const host = draft?.hosts[Number(input.dataset[key])];
          if (host)
            host[field] = input.checked;
        });
      });
    }
    body.querySelectorAll("[data-remove-host]").forEach((button) => {
      button.addEventListener("click", () => {
        draft?.hosts.splice(Number(button.dataset.removeHost), 1);
        render();
      });
    });
    body.querySelectorAll("[data-remove-route]").forEach((button) => {
      button.addEventListener("click", () => {
        const [hostIndex, routeIndex] = String(button.dataset.removeRoute).split(":").map(Number);
        draft?.hosts[hostIndex]?.routes.splice(routeIndex, 1);
        render();
      });
    });
    body.querySelectorAll("[data-add-route]").forEach((form) => {
      form.addEventListener("submit", (event) => {
        event.preventDefault();
        const index = Number(form.dataset.addRoute);
        const host = draft?.hosts[index];
        if (!host)
          return;
        const data = new FormData(form);
        const redirect = String(data.get("redirect") ?? "").trim();
        const upstream = String(data.get("upstream") ?? "").trim();
        const limit = String(data.get("limit") ?? "").trim();
        const route = {
          id: uniqueId(host.routes.map((route) => route.id), "route"),
          match: String(data.get("match") ?? "").trim(),
          upstream: redirect ? null : upstream || null,
          "strip-prefix": data.get("strip") === "on",
          websocket: data.get("websocket") === "on",
          redirect: redirect || null,
          headers: [],
          "body-limit": limit ? Number(limit) : null
        };
        host.routes.push(route);
        render();
      });
    });
    body.querySelector("[data-add-host]")?.addEventListener("click", () => {
      addHost();
    });
  }
  function renderLogs() {
    body.innerHTML = `<div class="px-bar"><span class="px-note">One event per request; the stream drops under budget and reports the count.</span><span class="grow"></span><span class="px-badge" data-log-count>${events.length} events</span><span class="px-badge">dropped ${status?.dropped ?? 0}</span><button type="button" data-clear-log>Clear</button></div>
      <div class="px-log" data-log data-testid="proxy-log">${events.map(eventRow).join("")}</div>`;
    body.querySelector("[data-clear-log]")?.addEventListener("click", () => {
      events = [];
      render();
    });
    startLogs();
  }
  async function addHost() {
    if (!context.openModal) {
      setStatus("This panel has no modal slot; add the host in the document directly.", true);
      return;
    }
    const opened = await context.openModal({ tag: "proxy-host-modal" });
    if (!opened)
      setStatus("The panel refused to open the add-host modal.", true);
  }
  function receiveHost(host) {
    if (!draft)
      return;
    if (draft.hosts.some((candidate) => candidate.id === host.id)) {
      setStatus(`A host called ${host.id} already exists.`, true);
      return;
    }
    draft.hosts.push(host);
    section = "hosts";
    render();
    setStatus(`Added ${host.id} to the draft; apply to publish it.`);
  }
  pendingHost = receiveHost;
  box.querySelectorAll("[data-section]").forEach((button) => {
    button.addEventListener("click", () => {
      section = button.dataset.section ?? "listeners";
      render();
    });
  });
  box.querySelector("[data-refresh]")?.addEventListener("click", () => {
    load();
  });
  box.querySelector("[data-apply]")?.addEventListener("click", () => {
    apply();
  });
  box.querySelector("[data-reload-certs]")?.addEventListener("click", () => {
    reloadCertificates();
  });
  const offAction = context.onAction?.((tag) => {
    if (tag === "apply")
      apply();
    else if (tag === "refresh")
      load();
    else if (tag === "add-host")
      addHost();
  });
  const offVisibility = context.onVisibility?.((visible) => {
    if (!visible) {
      stopLogs();
      return;
    }
    if (section === "log")
      startLogs();
    else
      refreshStatus();
  });
  const initial = context.params?.addHost;
  load().then(() => {
    if (!disposed && initial)
      receiveHost(initial);
  });
  return () => {
    disposed = true;
    pendingHost = null;
    offAction?.();
    offVisibility?.();
    stopLogs();
    root.replaceChildren();
  };
}
export {
  mount,
  onParams
};
