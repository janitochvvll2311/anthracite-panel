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

// src/styles.ts
var styles = `
  .ct { display: flex; flex-direction: column; gap: 14px; }
  .ct-bar { display: flex; align-items: center; gap: 8px; }
  .ct-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; color: #8f98ab; font-size: 12px; }
  .grow { flex: 1; }
  .ct-bar button, .ct-actions button {
    padding: 6px 11px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .ct-bar button:hover, .ct-actions button:hover { border-color: #4a5c82; color: #fff; }
  .ct-summary { display: flex; flex-wrap: wrap; gap: 8px; }
  .ct-badge {
    display: inline-flex; align-items: center; gap: 5px; padding: 3px 8px;
    border-radius: 999px; border: 1px solid #2a3142; background: #11151d;
    color: #98a2b6; font-size: 11px;
  }
  .ct-badge.ok { border-color: #2f5a3f; color: #93d9ab; background: #122018; }
  .ct-badge.warn { border-color: #5a4a2a; color: #f3c969; background: #221c10; }
  .ct-badge.bad { border-color: #5c2b31; color: #ffb3ba; background: #241214; }
  .ct-badge.off { color: #7a8497; }
  .ct-table { display: flex; flex-direction: column; gap: 4px; }
  .ct-row {
    display: grid; grid-template-columns: minmax(160px, 2fr) 90px 80px 90px 90px 60px;
    gap: 10px; align-items: center; width: 100%; text-align: left;
    padding: 8px 10px; border-radius: 7px; border: 1px solid transparent;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 12.5px; cursor: pointer;
  }
  .ct-row:hover { border-color: #2f3a52; }
  .ct-row.selected { border-color: #4a5c82; background: #151d2c; }
  .ct-row.head { background: transparent; color: #7a8497; font-size: 11px; cursor: default; }
  .ct-row.head:hover { border-color: transparent; }
  .ct-empty { color: #7a8497; font-size: 12.5px; }
  .ct-detail { display: flex; flex-direction: column; gap: 10px; padding: 12px; border-radius: 8px; border: 1px solid #2a3142; background: #0f1219; }
  .ct-detail h3 { margin: 0; font-size: 16px; font-weight: 600; }
  .ct-detail dl { display: grid; grid-template-columns: 120px 1fr; gap: 6px 12px; margin: 0; font-size: 12px; }
  .ct-detail dt { color: #7a8497; }
  .ct-detail dd { margin: 0; color: #d6dcea; overflow-wrap: anywhere; }
  .ct-actions { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .ct-actions .danger { border-color: #5c2b31; color: #ffb3ba; }
  .ct-check { display: inline-flex; align-items: center; gap: 5px; color: #98a2b6; font-size: 12px; }
  .ct-status { min-height: 16px; color: #667087; font-size: 11.5px; margin: 0; }
  .ct-status.bad { color: #ffb3ba; }
  @media (max-width: 700px) {
    .ct-row { grid-template-columns: 1fr 80px 70px; }
    .ct-row > :nth-child(4), .ct-row > :nth-child(5), .ct-row > :nth-child(6) { display: none; }
  }
`;

// src/main.ts
function date(seconds) {
  if (!seconds)
    return "—";
  return new Date(seconds * 1000).toLocaleString();
}
function days(seconds) {
  return Math.ceil((seconds * 1000 - Date.now()) / 86400000);
}
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = "ct";
  box.innerHTML = `
    <div class="ct-bar">
      <span class="ct-mono">managed certificates and scan roots</span>
      <span class="grow"></span>
      <button type="button" data-import>Import</button>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="ct-summary" data-summary></div>
    <div class="ct-table" role="table" aria-label="Certificates"></div>
    <section class="ct-detail" data-detail hidden></section>
    <p class="ct-status" role="status"></p>
  `;
  root.append(box);
  const table = box.querySelector(".ct-table");
  const summary = box.querySelector("[data-summary]");
  const detail = box.querySelector("[data-detail]");
  const status = box.querySelector(".ct-status");
  const importButton = box.querySelector("[data-import]");
  const refresh = box.querySelector("[data-refresh]");
  const hasFab = context.panel?.slots?.includes("panel.fab") === true;
  const hasModal = context.panel?.slots?.includes("panel.modal") === true;
  if (!hasFab || !hasModal) {
    importButton.hidden = true;
  }
  let certificates = [];
  let selected = null;
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "ct-status bad" : "ct-status";
  };
  const badge = (label, kind) => {
    const node = document.createElement("span");
    node.className = `ct-badge ${kind}`;
    node.textContent = label;
    return node;
  };
  const statusKind = (status) => {
    if (status === "valid")
      return "ok";
    if (status === "expiring")
      return "warn";
    if (status === "expired")
      return "bad";
    return "off";
  };
  function renderSummary() {
    const expiring = certificates.filter((cert) => cert.status === "expiring").length;
    const expired = certificates.filter((cert) => cert.status === "expired").length;
    summary.replaceChildren(badge(`${certificates.length} certificates`, "off"), badge(`${expiring} expiring`, expiring ? "warn" : "off"), badge(`${expired} expired`, expired ? "bad" : "off"));
  }
  function renderDetail(cert) {
    selected = cert.id;
    detail.hidden = false;
    detail.replaceChildren();
    const title = document.createElement("h3");
    title.textContent = cert.names[0] ?? cert.id;
    const grid = document.createElement("dl");
    const row = (label, value) => {
      const dt = document.createElement("dt");
      dt.textContent = label;
      const dd = document.createElement("dd");
      dd.textContent = value;
      grid.append(dt, dd);
    };
    row("Issuer", cert.issuer);
    row("Names", cert.names.join(", ") || "—");
    row("Validity", `${date(cert["not-before"])} → ${date(cert["not-after"])}`);
    row("Key", cert["key-type"]);
    row("Source", cert.source);
    row("Fingerprint", cert["fingerprint-sha256"]);
    row("Status", cert.status);
    const actions = document.createElement("div");
    actions.className = "ct-actions";
    const validate = document.createElement("button");
    validate.type = "button";
    validate.textContent = "Validate";
    const publish = document.createElement("button");
    publish.type = "button";
    publish.textContent = "Publish";
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Remove";
    remove.className = "danger";
    const close = document.createElement("button");
    close.type = "button";
    close.textContent = "Close";
    if (cert["fingerprint-sha256"]) {
      actions.append(validate, publish, remove);
    }
    actions.append(close);
    if (cert.source === "acme") {
      const renew = document.createElement("label");
      renew.className = "ct-check";
      const input = document.createElement("input");
      input.type = "checkbox";
      input.checked = cert["auto-renew"];
      input.addEventListener("change", async () => {
        try {
          await call(context, "/certificate/auto-renew", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ id: cert.id, enabled: input.checked })
          });
          setStatus(input.checked ? "Automatic renewal enabled." : "Automatic renewal disabled.");
          await load();
        } catch (error) {
          input.checked = !input.checked;
          setStatus(String(error), true);
        }
      });
      renew.append(input, document.createTextNode(" auto-renew"));
      actions.append(renew);
    }
    detail.append(title, grid, actions);
    validate.addEventListener("click", async () => {
      setStatus("Validating…");
      try {
        const result = await call(context, "/certificate/validate", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id: cert.id })
        });
        setStatus(result.ok ? "Valid: the chain and key are consistent." : `Invalid: ${result.reason ?? "unknown reason"}`, !result.ok);
      } catch (error) {
        setStatus(String(error), true);
      }
    });
    publish.addEventListener("click", async () => {
      setStatus("Publishing…");
      try {
        await call(context, "/certificate/publish", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id: cert.id })
        });
        setStatus("Published to {shared}/tls.");
      } catch (error) {
        setStatus(String(error), true);
      }
    });
    let confirming = false;
    remove.addEventListener("click", async () => {
      if (!confirming) {
        confirming = true;
        remove.textContent = "Confirm remove";
        setStatus("The published copy stays until the proxy reloads.");
        return;
      }
      try {
        await call(context, `/certificate?id=${encodeURIComponent(cert.id)}`, {
          method: "DELETE"
        });
        setStatus("Removed from the managed store; the published copy is kept.");
        detail.hidden = true;
        await load();
      } catch (error) {
        setStatus(String(error), true);
      }
    });
    close.addEventListener("click", () => {
      detail.hidden = true;
      selected = null;
    });
  }
  function render() {
    renderSummary();
    table.replaceChildren();
    if (certificates.length === 0) {
      const empty = document.createElement("p");
      empty.className = "ct-empty";
      empty.textContent = "No certificates found in the scan roots or the managed store.";
      table.append(empty);
      return;
    }
    const head = document.createElement("div");
    head.className = "ct-row head";
    for (const label of ["Name", "Status", "Expires", "Source", "Key", "Renew"]) {
      const cell = document.createElement("span");
      cell.textContent = label;
      head.append(cell);
    }
    table.append(head);
    for (const cert of certificates) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "ct-row";
      if (cert.id === selected)
        row.classList.add("selected");
      const name = document.createElement("span");
      name.textContent = cert.names[0] ?? cert.id;
      const state = document.createElement("span");
      state.append(badge(cert.status, statusKind(cert.status)));
      const expiry = document.createElement("span");
      const left = days(cert["not-after"]);
      expiry.textContent = left < 0 ? "expired" : `${left} d`;
      const source = document.createElement("span");
      source.textContent = cert.source;
      const key = document.createElement("span");
      key.textContent = cert["key-type"];
      const renew = document.createElement("span");
      renew.textContent = cert["auto-renew"] ? "on" : "—";
      row.append(name, state, expiry, source, key, renew);
      row.addEventListener("click", () => renderDetail(cert));
      table.append(row);
    }
  }
  async function load() {
    setStatus("Reading inventory…");
    try {
      const list = await call(context, "/certs");
      if (disposed)
        return;
      certificates = Array.isArray(list) ? list : [];
      render();
      if (selected) {
        const current = certificates.find((cert) => cert.id === selected);
        if (current)
          renderDetail(current);
        else
          detail.hidden = true;
      }
      const errors = certificates.filter((cert) => cert.status === "invalid").length;
      setStatus(`Read ${certificates.length} certificate(s)${errors ? `; ${errors} invalid` : ""}.`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  importButton.addEventListener("click", () => {
    context.openModal?.({ tag: "certs-import-modal" });
  });
  refresh.addEventListener("click", () => void load());
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh")
      load();
  });
  const offVisibility = context.onVisibility?.((visible) => {
    if (visible)
      load();
  });
  load();
  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    root.replaceChildren();
  };
}
export {
  mount
};
