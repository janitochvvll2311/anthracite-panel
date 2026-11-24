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
  .pxm input[type="text"], .pxm select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 13px;
  }
  .pxm-row { display: flex; gap: 14px; flex-wrap: wrap; }
  .pxm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .pxm button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .pxm button.primary { border-color: #33507a; background: #16233a; color: #dbe6ff; }
  .pxm-status { margin: 0; font-size: 12px; color: #8f98ab; }
  .pxm-status.bad { color: #ffb3ba; }
`;
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);
  const box = document.createElement("div");
  box.className = "pxm";
  box.innerHTML = `
    <label>Id<input type="text" name="id" placeholder="site" autofocus required /></label>
    <label>Names<input type="text" name="names" placeholder="example.com, www.example.com" required /></label>
    <label>Certificate<select name="certificate"><option value="">(none)</option></select></label>
    <div class="pxm-row">
      <label class="px-check"><input type="checkbox" name="acme" /> serve ACME HTTP-01</label>
      <label class="px-check"><input type="checkbox" name="redirect" /> redirect HTTP to HTTPS</label>
      <label class="px-check"><input type="checkbox" name="hsts" /> HSTS</label>
    </div>
    <div class="pxm-actions">
      <button type="button" data-cancel>Cancel</button>
      <button type="submit" class="primary" data-submit>Add to draft</button>
    </div>
    <p class="pxm-status" role="status" data-status></p>
  `;
  root.append(box);
  const certificate = box.querySelector('select[name="certificate"]');
  const status = box.querySelector("[data-status]");
  const submit = box.querySelector("[data-submit]");
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message;
    status.className = bad ? "pxm-status bad" : "pxm-status";
  };
  const value = (name) => {
    const input = box.querySelector(`[name="${name}"]`);
    return input?.value.trim() ?? "";
  };
  const checked = (name) => box.querySelector(`[name="${name}"]`)?.checked ?? false;
  async function loadCertificates() {
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
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function commit() {
    const id = value("id");
    const names = value("names").split(",").map((name) => name.trim().toLowerCase()).filter(Boolean);
    if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
      setStatus("The id is lowercase letters, digits and dashes.", true);
      return;
    }
    if (names.length === 0) {
      setStatus("At least one host name is required.", true);
      return;
    }
    const host = {
      id,
      names,
      certificate: certificate.value || null,
      acme: checked("acme"),
      "redirect-https": checked("redirect"),
      hsts: checked("hsts"),
      routes: []
    };
    submit.disabled = true;
    setStatus("Handing the host to the view…");
    try {
      const opened = await context.openView?.({
        tag: "anth-proxy",
        params: { addHost: host }
      });
      if (!opened) {
        submit.disabled = false;
        setStatus("The panel did not hand the host to the view.", true);
        return;
      }
      if (!disposed)
        context.close();
    } catch (error) {
      submit.disabled = false;
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  submit.addEventListener("click", () => void commit());
  box.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      commit();
    }
  });
  box.querySelector("[data-cancel]")?.addEventListener("click", () => context.close());
  loadCertificates();
  box.querySelector('[name="id"]')?.focus();
  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
export {
  mount
};
