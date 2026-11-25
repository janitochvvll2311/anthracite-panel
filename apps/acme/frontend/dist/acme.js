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
async function watchOrder(context, order, signal, onEvent) {
  const response = await context.fetch(route(context, `/order/events?order=${encodeURIComponent(order)}`), { headers: { accept: "text/event-stream" }, signal });
  if (!response.ok) {
    throw new Error(await response.text() || `HTTP ${response.status}`);
  }
  if (!response.body)
    throw new Error("the timeline has no body");
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

// src/state.ts
function badgeClass(status) {
  if (status === "valid")
    return "ok";
  if (status === "pending" || status === "ready" || status === "processing") {
    return "warn";
  }
  if (status === "invalid" || status === "revoked")
    return "bad";
  if (status === "issued-unstored")
    return "warn";
  return "off";
}
function directoryLabel(url) {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
function isStaging(capabilities, url) {
  return capabilities.some((capability) => (capability["staging-directories"] ?? []).includes(url));
}

// src/styles.ts
var styles = `
  .ac { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .ac-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .ac-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .ac-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .ac button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .ac button:hover { border-color: var(--accent); }
  .ac button.primary { border-color: var(--accent); background: var(--panel-3); }
  .ac button.danger { border-color: var(--danger); color: var(--danger); }
  .ac button:disabled { opacity: .55; cursor: default; }
  .ac-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .ac-status.bad { color: var(--danger); }
  .ac-section { display: flex; flex-direction: column; gap: 8px; }
  .ac-section-head { display: flex; align-items: center; gap: 8px; }
  .ac-section-head strong { font-size: 12px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .ac-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; }
  .ac-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0;
  }
  .ac-card.selectable { cursor: pointer; }
  .ac-card.selectable:hover { border-color: var(--line); }
  .ac-card.selected { border-color: var(--accent); }
  .ac-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .ac-card-head strong {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12.5px;
    color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ac-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .ac-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .ac-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .ac-badge.ok { border-color: var(--good); color: var(--good); }
  .ac-badge.warn { border-color: var(--warn); color: var(--warn); }
  .ac-badge.bad { border-color: var(--danger); color: var(--danger); }
  .ac-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px;
    color: var(--muted); font-size: 12px;
  }
  .ac-banner {
    padding: 8px 10px; border-radius: 8px; border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, var(--panel)); color: var(--danger); font-size: 12px;
  }
  .ac-note { font-size: 12px; color: var(--muted); }
  .ac-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .ac-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .ac-detail-head strong { font-size: 13px; color: var(--text); }
  .ac-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .ac-timeline { display: flex; flex-direction: column; gap: 0; }
  .ac-event {
    display: grid; grid-template-columns: 150px 120px minmax(0, 1fr); gap: 10px;
    padding: 6px 4px; border-bottom: 1px solid var(--line-soft); font-size: 12px;
  }
  .ac-event:last-child { border-bottom: 0; }
  .ac-event time { color: var(--muted); font-variant-numeric: tabular-nums; }
  .ac-event code { color: var(--accent); font-size: 11.5px; }
  .ac-event span { color: var(--text); overflow-wrap: anywhere; }
  .ac-confidence { font-size: 11.5px; color: var(--muted); }
  .ac-dialog {
    border: 1px solid var(--line); border-radius: 12px; padding: 14px;
    background: var(--panel); color: var(--text); width: min(420px, calc(100vw - 32px));
  }
  .ac-dialog::backdrop { background: #05070bb3; }
  .ac-dialog h3 { margin: 0 0 10px; font-size: 13px; }
  .ac-dialog label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--muted); }
  .ac-dialog select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 13px;
  }
  .ac-dialog .ac-actions { justify-content: flex-end; margin-top: 12px; }

  html[data-viewport="tablet"] .ac-grid { grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); }
  html[data-viewport="tablet"] .ac-event { grid-template-columns: 130px 110px minmax(0, 1fr); }

  html[data-viewport="mobile"] .ac-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .ac-event { grid-template-columns: minmax(0, 1fr); gap: 2px; }
  html[data-viewport="mobile"] .ac-actions { width: 100%; }
  html[data-viewport="mobile"] .ac-actions button { flex: 1 1 auto; }
  html[data-viewport="mobile"] .ac-detail { padding: 10px; }

  @container (max-width: 640px) {
    .ac-grid { grid-template-columns: minmax(0, 1fr); }
    .ac-event { grid-template-columns: minmax(0, 1fr); gap: 2px; }
    .ac-actions button { flex: 1 1 auto; }
  }
`;

// src/main.ts
function date(seconds) {
  if (!seconds)
    return "—";
  return new Date(seconds * 1000).toLocaleString();
}
function escape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = "ac";
  box.dataset.testid = "acme-view";
  box.innerHTML = `
    <div class="ac-bar">
      <span class="ac-title">ACME</span>
      <span class="ac-badge" data-counts></span>
      <span class="grow"></span>
      <button type="button" data-refresh data-testid="acme-refresh">Refresh</button>
    </div>
    <p class="ac-status" role="status" data-status data-testid="acme-status"></p>
    <section class="ac-section">
      <div class="ac-section-head"><strong>Accounts</strong></div>
      <div class="ac-grid" data-accounts data-testid="acme-accounts"></div>
    </section>
    <section class="ac-section">
      <div class="ac-section-head">
        <strong>Orders</strong>
        <span class="ac-note" data-live></span>
      </div>
      <div class="ac-grid" data-orders data-testid="acme-orders"></div>
    </section>
    <section class="ac-detail" data-detail data-testid="acme-detail" hidden></section>
    <p class="ac-note" data-readonly hidden>
      This panel has no modal slot: issue certificates through the CLI or the
      API. Orders and the timeline stay visible here.
    </p>
    <dialog class="ac-dialog" data-revoke>
      <h3>Revoke certificate</h3>
      <label>Reason
        <select data-revoke-reason>
          <option value="superseded">superseded</option>
          <option value="key-compromise">key compromise</option>
          <option value="cessation-of-operation">cessation of operation</option>
          <option value="affiliation-changed">affiliation changed</option>
          <option value="unspecified">unspecified</option>
        </select>
      </label>
      <div class="ac-actions">
        <button type="button" data-revoke-cancel>Cancel</button>
        <button type="button" class="danger" data-revoke-confirm data-testid="acme-revoke-confirm">Revoke</button>
      </div>
    </dialog>
  `;
  root.append(box);
  const status = box.querySelector("[data-status]");
  const counts = box.querySelector("[data-counts]");
  const accountsNode = box.querySelector("[data-accounts]");
  const ordersNode = box.querySelector("[data-orders]");
  const detailNode = box.querySelector("[data-detail]");
  const liveNode = box.querySelector("[data-live]");
  const readonly = box.querySelector("[data-readonly]");
  const dialog = box.querySelector("[data-revoke]");
  const hasFab = context.panel?.slots?.includes("panel.fab") === true;
  const hasModal = context.panel?.slots?.includes("panel.modal") === true;
  if (!hasFab || !hasModal)
    readonly.hidden = false;
  let accounts = [];
  let orders = [];
  let capabilities = [];
  let selected = null;
  let events = [];
  let seen = new Set;
  let follow = null;
  let poll = null;
  let visible = true;
  let disposed = false;
  let revoking = null;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "ac-status bad" : "ac-status";
  };
  function renderCounts() {
    const active = orders.filter((order) => ["pending", "ready", "processing"].includes(order.status)).length;
    const valid = orders.filter((order) => order.status === "valid").length;
    counts.textContent = `${accounts.length} accounts · ${active} active · ${valid} valid`;
  }
  function renderAccounts() {
    accountsNode.replaceChildren();
    if (accounts.length === 0) {
      const empty = document.createElement("div");
      empty.className = "ac-empty";
      empty.textContent = "No account yet. The Issue certificate form creates one for the chosen directory.";
      accountsNode.append(empty);
      return;
    }
    for (const account of accounts) {
      const card = document.createElement("div");
      card.className = "ac-card";
      card.dataset.testid = "acme-account";
      card.dataset.accountId = account.id;
      const staging = isStaging(capabilities, account.directory);
      card.innerHTML = `
        <div class="ac-card-head">
          <strong title="${escape(account.id)}">${escape(account.id)}</strong>
          <span class="grow"></span>
          <span class="ac-badge ${staging ? "warn" : "ok"}">${staging ? "staging" : "production"}</span>
          <span class="ac-badge">${escape(account.status)}</span>
        </div>
        <div class="ac-field"><span class="ac-field-name">Directory</span><span class="ac-mono" title="${escape(account.directory)}">${escape(directoryLabel(account.directory))}</span></div>
        <div class="ac-field"><span class="ac-field-name">Contact</span><span>${escape(account.contact.join(", ") || "(none)")}</span></div>
        <div class="ac-field"><span class="ac-field-name">Key</span><span class="ac-mono">${escape(account["key-type"])}</span></div>
      `;
      accountsNode.append(card);
    }
  }
  function renderOrders() {
    ordersNode.replaceChildren();
    if (orders.length === 0) {
      const empty = document.createElement("div");
      empty.className = "ac-empty";
      empty.textContent = "No order yet. Use the FAB or the API to request a certificate.";
      ordersNode.append(empty);
      return;
    }
    for (const order of orders) {
      const card = document.createElement("div");
      card.className = `ac-card selectable${order.id === selected ? " selected" : ""}`;
      card.dataset.testid = "acme-order";
      card.dataset.orderId = order.id;
      card.dataset.status = order.status;
      card.innerHTML = `
        <div class="ac-card-head">
          <strong title="${escape(order.names.join(", "))}">${escape(order.names[0] ?? order.id)}</strong>
          <span class="grow"></span>
          <span class="ac-badge ${badgeClass(order.status)}" data-testid="acme-order-status">${escape(order.status)}</span>
        </div>
        <div class="ac-field"><span class="ac-field-name">Names</span><span>${escape(order.names.join(", "))}</span></div>
        <div class="ac-field"><span class="ac-field-name">Challenge</span><span class="ac-mono">${escape(order.challenge)}</span></div>
        <div class="ac-field"><span class="ac-field-name">Expires</span><span>${escape(date(order["expires-at"]))}</span></div>
        ${order.certificate ? `<div class="ac-field"><span class="ac-field-name">Certificate</span><span class="ac-mono">${escape(order.certificate)}</span></div>` : ""}
        ${order["last-error"] ? `<div class="ac-banner">${escape(order["last-error"])}</div>` : ""}
      `;
      card.addEventListener("click", () => select(order.id));
      ordersNode.append(card);
    }
  }
  function renderEvents() {
    const timeline = detailNode.querySelector("[data-timeline]");
    if (!timeline)
      return;
    timeline.replaceChildren();
    for (const event of events) {
      const row = document.createElement("div");
      row.className = "ac-event";
      row.dataset.testid = "acme-event";
      row.dataset.kind = event.kind;
      row.innerHTML = `
        <time>${escape(date(event.at))}</time>
        <code>${escape(event.kind)}</code>
        <span>${escape(event.detail ?? "")}</span>
      `;
      timeline.append(row);
    }
  }
  function renderDetail() {
    const order = orders.find((entry) => entry.id === selected);
    if (!order) {
      detailNode.hidden = true;
      detailNode.replaceChildren();
      return;
    }
    detailNode.hidden = false;
    detailNode.replaceChildren();
    const head = document.createElement("div");
    head.className = "ac-detail-head";
    head.innerHTML = `
      <strong>${escape(order.names[0] ?? order.id)}</strong>
      <span class="ac-badge ${badgeClass(order.status)}">${escape(order.status)}</span>
      <span class="ac-mono">${escape(order.id)}</span>
    `;
    const actions = document.createElement("div");
    actions.className = "ac-actions";
    if (order.challenge === "dns-01" && order.status === "pending") {
      const confirm = document.createElement("button");
      confirm.type = "button";
      confirm.className = "primary";
      confirm.textContent = "Confirm DNS record";
      confirm.dataset.testid = "acme-confirm";
      confirm.addEventListener("click", () => void confirmOrder(order));
      actions.append(confirm);
    }
    if (order.certificate && ["valid", "issued-unstored"].includes(order.status)) {
      const renew = document.createElement("button");
      renew.type = "button";
      renew.textContent = "Renew";
      renew.dataset.testid = "acme-renew";
      renew.addEventListener("click", () => void renewOrder(order));
      const revoke = document.createElement("button");
      revoke.type = "button";
      revoke.className = "danger";
      revoke.textContent = "Revoke";
      revoke.dataset.testid = "acme-revoke";
      revoke.addEventListener("click", () => openRevoke(order));
      actions.append(renew, revoke);
    }
    const close = document.createElement("button");
    close.type = "button";
    close.textContent = "Close";
    close.addEventListener("click", () => {
      selected = null;
      events = [];
      seen = new Set;
      stopFollow();
      render();
    });
    actions.append(close);
    const facts = document.createElement("div");
    facts.className = "ac-confidence";
    facts.textContent = `Account ${order.account} · ${order.challenge} · expires ${date(order["expires-at"])}`;
    const timeline = document.createElement("div");
    timeline.className = "ac-timeline";
    timeline.dataset.testid = "acme-timeline";
    timeline.dataset.timeline = "";
    const title = document.createElement("div");
    title.className = "ac-field-name";
    title.textContent = "Timeline";
    detailNode.append(head, facts, actions, title, timeline);
    if (order["last-error"]) {
      const banner = document.createElement("div");
      banner.className = "ac-banner";
      banner.textContent = order["last-error"];
      detailNode.insertBefore(banner, title);
    }
    renderEvents();
  }
  function render() {
    renderCounts();
    renderAccounts();
    renderOrders();
    renderDetail();
  }
  async function load() {
    try {
      const [nextAccounts, nextOrders, nextCapabilities] = await Promise.all([
        call(context, "/accounts"),
        call(context, "/orders"),
        call(context, "/capabilities")
      ]);
      accounts = nextAccounts ?? [];
      orders = nextOrders ?? [];
      capabilities = nextCapabilities ?? [];
      if (selected && !orders.some((order) => order.id === selected)) {
        selected = null;
        stopFollow();
      }
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  function select(id) {
    if (selected === id)
      return;
    selected = id;
    events = [];
    seen = new Set;
    render();
    startFollow();
  }
  function startFollow() {
    stopFollow();
    if (!selected)
      return;
    const id = selected;
    const controller = new AbortController;
    follow = controller;
    liveNode.textContent = "live";
    watchOrder(context, id, controller.signal, (event) => {
      const key = `${event.at}|${event.kind}|${event.detail ?? ""}`;
      if (seen.has(key))
        return;
      seen.add(key);
      events.push(event);
      renderEvents();
      if (["issued", "renewed", "failed", "revoked"].includes(event.kind)) {
        load();
      }
    }).catch(() => {
      if (controller.signal.aborted || disposed)
        return;
      liveNode.textContent = "polling (the live timeline ended)";
    }).finally(() => {
      if (follow === controller)
        follow = null;
    });
  }
  function stopFollow() {
    follow?.abort();
    follow = null;
    liveNode.textContent = "";
  }
  async function confirmOrder(order) {
    try {
      setStatus(`Confirming ${order.names[0]}…`);
      await call(context, "/order/confirm", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ order: order.id })
      });
      setStatus("The CA is validating the DNS record.");
      await load();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function renewOrder(order) {
    try {
      setStatus(`Renewing ${order.names[0]}…`);
      const next = await call(context, "/order/renew", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ certificate: order.certificate })
      });
      select(next.id);
      setStatus("The renewal order started.");
      await load();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  function openRevoke(order) {
    revoking = order.certificate ?? null;
    dialog.showModal();
  }
  async function revoke() {
    if (!revoking)
      return;
    const reason = dialog.querySelector("[data-revoke-reason]").value;
    try {
      setStatus("Revoking…");
      await call(context, "/order/revoke", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ certificate: revoking, reason })
      });
      dialog.close();
      setStatus("The CA accepted the revocation.");
      await load();
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  box.querySelector("[data-refresh]")?.addEventListener("click", () => {
    load();
  });
  box.querySelector("[data-revoke-cancel]")?.addEventListener("click", () => {
    revoking = null;
    dialog.close();
  });
  box.querySelector("[data-revoke-confirm]")?.addEventListener("click", () => {
    revoke();
  });
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh")
      load();
  });
  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    if (next) {
      load();
    }
  });
  const initial = context.params;
  load().then(() => {
    if (!disposed && initial?.order)
      select(initial.order);
  });
  poll = window.setInterval(() => {
    if (visible && !disposed)
      load();
  }, 5000);
  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    stopFollow();
    if (poll !== null)
      window.clearInterval(poll);
    root.replaceChildren();
  };
}
export {
  mount
};
