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

// src/modal.ts
var extra = `
  .pxm { display: flex; flex-direction: column; gap: 12px; }
  .pxm label { display: flex; flex-direction: column; gap: 4px; color: #98a2b6; font-size: 12px; }
  .pxm input[type="text"], .pxm input[type="number"], .pxm select {
    width: 100%; padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 13px;
  }
  .pxm input:disabled { color: #7a8497; }
  .pxm-row { display: flex; gap: 14px; flex-wrap: wrap; }
  .pxm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .pxm button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .pxm button.primary { border-color: #33507a; background: #16233a; color: #dbe6ff; }
  .pxm-status { margin: 0; font-size: 12px; color: #8f98ab; }
  .pxm-status.bad { color: #ffb3ba; }
  @media (max-width: 720px) {
    .pxm-actions { flex-direction: column-reverse; }
    .pxm-actions button { width: 100%; }
  }
`;
function frame(root) {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);
  const box = document.createElement("div");
  box.className = "pxm";
  root.append(box);
  return { style, box };
}
function status(box) {
  return box.querySelector("[data-status]");
}
function setStatus(box, message, bad = false) {
  const node = status(box);
  node.textContent = message;
  node.className = bad ? "pxm-status bad" : "pxm-status";
}
function text(box, name) {
  return box.querySelector(`[name="${name}"]`)?.value.trim() ?? "";
}
function number(box, name, fallback) {
  const value = Number(text(box, name));
  return Number.isFinite(value) && value > 0 ? value : fallback;
}
function checked(box, name) {
  return box.querySelector(`[name="${name}"]`)?.checked ?? false;
}
function escape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
async function loadConfig(context) {
  return await call(context, "/config");
}
function actions() {
  return `<div class="pxm-actions">
    <button type="button" data-cancel data-testid="proxy-modal-cancel">Cancel</button>
    <button type="button" class="primary" data-submit data-testid="proxy-modal-submit">Save</button>
  </div>
  <p class="pxm-status" role="status" data-status data-testid="proxy-modal-status"></p>`;
}
function wire(box, context, commit) {
  const submit = box.querySelector("[data-submit]");
  box.querySelector("[data-cancel]")?.addEventListener("click", () => context.close());
  submit.addEventListener("click", () => void commit());
  box.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      commit();
    }
  });
}
async function deliver(box, context, params) {
  const submit = box.querySelector("[data-submit]");
  submit.disabled = true;
  const opened = await context.openView?.({ tag: "anth-proxy", params });
  if (!opened) {
    submit.disabled = false;
    setStatus(box, "The panel did not hand the draft back to the view.", true);
    return;
  }
  context.close();
}
function hostForm(box, context) {
  const host = context.params?.host;
  let disposed = false;
  box.innerHTML = `
    <label>Id<input type="text" name="id" data-testid="proxy-host-id" value="${escape(host?.id ?? "")}" ${host ? "disabled" : ""} placeholder="site" /></label>
    <label>Names<input type="text" name="names" data-testid="proxy-host-names" value="${escape(host?.names.join(", ") ?? "")}" placeholder="example.com, www.example.com" /></label>
    <label>Certificate<select name="certificate" data-testid="proxy-host-certificate"><option value="">(none)</option></select></label>
    <div class="pxm-row">
      <label class="px-check"><input type="checkbox" name="acme" data-testid="proxy-host-acme" ${host?.acme ? "checked" : ""} /> serve ACME HTTP-01</label>
      <label class="px-check"><input type="checkbox" name="redirect" data-testid="proxy-host-redirect" ${host?.["redirect-https"] ? "checked" : ""} /> redirect HTTP to HTTPS</label>
      <label class="px-check"><input type="checkbox" name="hsts" data-testid="proxy-host-hsts" ${host?.hsts ? "checked" : ""} /> HSTS</label>
    </div>
    ${actions()}`;
  const certificate = box.querySelector('select[name="certificate"]');
  (async () => {
    try {
      const ids = await call(context, "/certificates");
      if (disposed)
        return;
      for (const id of Array.isArray(ids) ? ids : []) {
        const option = document.createElement("option");
        option.value = id;
        option.textContent = id;
        certificate.append(option);
      }
      certificate.value = host?.certificate ?? "";
    } catch (error) {
      if (!disposed)
        setStatus(box, String(error), true);
    }
  })();
  wire(box, context, async () => {
    const id = text(box, "id");
    const names = text(box, "names").split(",").map((name) => name.trim().toLowerCase()).filter(Boolean);
    if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
      setStatus(box, "The id is lowercase letters, digits and dashes.", true);
      return;
    }
    if (names.length === 0) {
      setStatus(box, "At least one host name is required.", true);
      return;
    }
    const next = {
      id,
      names,
      certificate: certificate.value || null,
      acme: checked(box, "acme"),
      "redirect-https": checked(box, "redirect"),
      hsts: checked(box, "hsts"),
      routes: host?.routes ?? []
    };
    await deliver(box, context, { upsertHost: next });
  });
  box.querySelector('[name="id"]')?.focus();
  return () => {
    disposed = true;
  };
}
function upstreamForm(box, context) {
  const upstream = context.params?.upstream;
  box.innerHTML = `
    <label>Id<input type="text" name="id" data-testid="proxy-upstream-id" value="${escape(upstream?.id ?? "")}" ${upstream ? "disabled" : ""} placeholder="app" /></label>
    <label>URL<input type="text" name="url" data-testid="proxy-upstream-url" value="${escape(upstream?.url ?? "")}" placeholder="http://127.0.0.1:3000" /></label>
    <div class="pxm-row">
      <label>Connect ms<input type="number" min="1" max="300000" name="connect" data-testid="proxy-upstream-connect" value="${upstream?.["connect-timeout-ms"] ?? 2000}" /></label>
      <label>Read ms<input type="number" min="1" max="300000" name="read" data-testid="proxy-upstream-read" value="${upstream?.["read-timeout-ms"] ?? 5000}" /></label>
    </div>
    <label class="px-check"><input type="checkbox" name="verify" data-testid="proxy-upstream-verify" ${upstream ? upstream["tls-verify"] ? "checked" : "" : "checked"} /> verify the upstream certificate</label>
    ${actions()}`;
  wire(box, context, async () => {
    const id = text(box, "id");
    const url = text(box, "url");
    if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
      setStatus(box, "The id is lowercase letters, digits and dashes.", true);
      return;
    }
    if (!/^https?:\/\//.test(url)) {
      setStatus(box, "The URL starts with http:// or https://.", true);
      return;
    }
    const next = {
      id,
      url,
      "connect-timeout-ms": number(box, "connect", 2000),
      "read-timeout-ms": number(box, "read", 5000),
      "tls-verify": checked(box, "verify")
    };
    await deliver(box, context, { upsertUpstream: next });
  });
  box.querySelector('[name="id"]')?.focus();
  return () => {};
}
function routeForm(box, context) {
  const params = context.params;
  const route = params?.route;
  const hostId = params?.hostId ?? "";
  let disposed = false;
  box.innerHTML = `
    <label>Match<input type="text" name="match" data-testid="proxy-route-match" value="${escape(route?.match ?? "")}" placeholder="= /health or /" /></label>
    <label>Upstream<select name="upstream" data-testid="proxy-route-upstream"><option value="">(none)</option></select></label>
    <label>Redirect<input type="text" name="redirect" data-testid="proxy-route-redirect" value="${escape(route?.redirect ?? "")}" placeholder="optional https://… or /path" /></label>
    <div class="pxm-row">
      <label class="px-check"><input type="checkbox" name="strip" data-testid="proxy-route-strip" ${route?.["strip-prefix"] ? "checked" : ""} /> strip the prefix</label>
      <label class="px-check"><input type="checkbox" name="websocket" data-testid="proxy-route-websocket" ${route?.websocket ? "checked" : ""} /> WebSocket</label>
      <label>Body limit<input type="number" min="1" name="limit" data-testid="proxy-route-limit" value="${route?.["body-limit"] ?? ""}" placeholder="default" /></label>
    </div>
    ${actions()}`;
  const upstream = box.querySelector('select[name="upstream"]');
  (async () => {
    try {
      const ids = params?.upstreams ?? (await loadConfig(context)).upstreams.map((entry) => entry.id);
      if (disposed)
        return;
      for (const id of ids) {
        const option = document.createElement("option");
        option.value = id;
        option.textContent = id;
        upstream.append(option);
      }
      upstream.value = route?.upstream ?? "";
    } catch (error) {
      if (!disposed)
        setStatus(box, String(error), true);
    }
  })();
  wire(box, context, async () => {
    const match = text(box, "match");
    const redirect = text(box, "redirect");
    const selected = upstream.value;
    if (!match.startsWith("/") && !match.startsWith("= /")) {
      setStatus(box, "The match starts with `/` or `= /`.", true);
      return;
    }
    if (!redirect && !selected) {
      setStatus(box, "Choose an upstream or write a redirect.", true);
      return;
    }
    const limit = text(box, "limit");
    const next = {
      id: route?.id ?? "",
      match,
      upstream: redirect ? null : selected,
      "strip-prefix": checked(box, "strip"),
      websocket: checked(box, "websocket"),
      redirect: redirect || null,
      headers: route?.headers ?? [],
      "body-limit": limit ? Number(limit) : null
    };
    await deliver(box, context, { upsertRoute: { hostId, route: next } });
  });
  box.querySelector('[name="match"]')?.focus();
  return () => {
    disposed = true;
  };
}
function listenerForm(box, context) {
  const params = context.params;
  const listener = params?.listener;
  let disposed = false;
  box.innerHTML = `
    <label>Id<input type="text" value="${escape(listener?.id ?? "")}" disabled /></label>
    <label>Bind<input type="text" value="${escape(listener?.bind ?? "")}" disabled /></label>
    <label>Default host<select name="default-host" data-testid="proxy-listener-default"><option value="">(none)</option></select></label>
    <label class="px-check"><input type="checkbox" name="forwarded" data-testid="proxy-listener-forwarded" ${listener?.forwarded ? "checked" : ""} /> trust and set X-Forwarded-*</label>
    ${actions()}`;
  const select = box.querySelector('select[name="default-host"]');
  (async () => {
    try {
      const hosts = params?.hosts ?? (await loadConfig(context)).hosts.map((host) => ({
        id: host.id,
        names: host.names
      }));
      if (disposed)
        return;
      for (const host of hosts) {
        const option = document.createElement("option");
        option.value = host.id;
        option.textContent = `${host.id} (${host.names.join(", ")})`;
        select.append(option);
      }
      select.value = listener?.["default-host"] ?? "";
    } catch (error) {
      if (!disposed)
        setStatus(box, String(error), true);
    }
  })();
  wire(box, context, async () => {
    if (!listener) {
      setStatus(box, "The listener is missing.", true);
      return;
    }
    const next = {
      ...listener,
      "default-host": select.value || null,
      forwarded: checked(box, "forwarded")
    };
    await deliver(box, context, { upsertListener: next });
  });
  return () => {
    disposed = true;
  };
}
function mount(root, context) {
  const { box } = frame(root);
  let disposeInner;
  switch (context.contribution.tag) {
    case "proxy-upstream-modal":
      disposeInner = upstreamForm(box, context);
      break;
    case "proxy-route-modal":
      disposeInner = routeForm(box, context);
      break;
    case "proxy-listener-modal":
      disposeInner = listenerForm(box, context);
      break;
    default:
      disposeInner = hostForm(box, context);
  }
  return () => {
    disposeInner();
    root.replaceChildren();
  };
}
export {
  mount
};
