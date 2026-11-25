import { call, watchOrder } from "./api";
import { badgeClass, directoryLabel, isStaging } from "./state";
import { styles } from "./styles";
import type {
  Account,
  Capability,
  FrameContext,
  Order,
  OrderEvent,
} from "./types";

// The ACME view: accounts, orders and one live order timeline.

function date(seconds: number): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleString();
}

function escape(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function mount(root: HTMLElement, context: FrameContext) {
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

  const status = box.querySelector("[data-status]") as HTMLElement;
  const counts = box.querySelector("[data-counts]") as HTMLElement;
  const accountsNode = box.querySelector("[data-accounts]") as HTMLElement;
  const ordersNode = box.querySelector("[data-orders]") as HTMLElement;
  const detailNode = box.querySelector("[data-detail]") as HTMLElement;
  const liveNode = box.querySelector("[data-live]") as HTMLElement;
  const readonly = box.querySelector("[data-readonly]") as HTMLElement;
  const dialog = box.querySelector("[data-revoke]") as HTMLDialogElement;
  const hasFab = context.panel?.slots?.includes("panel.fab") === true;
  const hasModal = context.panel?.slots?.includes("panel.modal") === true;
  if (!hasFab || !hasModal) readonly.hidden = false;

  let accounts: Account[] = [];
  let orders: Order[] = [];
  let capabilities: Capability[] = [];
  let selected: string | null = null;
  let events: OrderEvent[] = [];
  let seen = new Set<string>();
  let follow: AbortController | null = null;
  let poll: number | null = null;
  let visible = true;
  let disposed = false;
  let revoking: string | null = null;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "ac-status bad" : "ac-status";
  };

  function renderCounts(): void {
    const active = orders.filter((order) =>
      ["pending", "ready", "processing"].includes(order.status),
    ).length;
    const valid = orders.filter((order) => order.status === "valid").length;
    counts.textContent = `${accounts.length} accounts · ${active} active · ${valid} valid`;
  }

  function renderAccounts(): void {
    accountsNode.replaceChildren();
    if (accounts.length === 0) {
      const empty = document.createElement("div");
      empty.className = "ac-empty";
      empty.textContent =
        "No account yet. The Issue certificate form creates one for the chosen directory.";
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

  function renderOrders(): void {
    ordersNode.replaceChildren();
    if (orders.length === 0) {
      const empty = document.createElement("div");
      empty.className = "ac-empty";
      empty.textContent =
        "No order yet. Use the FAB or the API to request a certificate.";
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

  function renderEvents(): void {
    const timeline = detailNode.querySelector("[data-timeline]");
    if (!timeline) return;
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

  function renderDetail(): void {
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
      seen = new Set();
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

  function render(): void {
    renderCounts();
    renderAccounts();
    renderOrders();
    renderDetail();
  }

  async function load(): Promise<void> {
    try {
      const [nextAccounts, nextOrders, nextCapabilities] = await Promise.all([
        call(context, "/accounts"),
        call(context, "/orders"),
        call(context, "/capabilities"),
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

  function select(id: string): void {
    if (selected === id) return;
    selected = id;
    events = [];
    seen = new Set();
    render();
    startFollow();
  }

  function startFollow(): void {
    stopFollow();
    if (!selected) return;
    const id = selected;
    const controller = new AbortController();
    follow = controller;
    liveNode.textContent = "live";
    void watchOrder(context, id, controller.signal, (event) => {
      const key = `${event.at}|${event.kind}|${event.detail ?? ""}`;
      if (seen.has(key)) return;
      seen.add(key);
      events.push(event);
      renderEvents();
      if (["issued", "renewed", "failed", "revoked"].includes(event.kind)) {
        void load();
      }
    })
      .catch(() => {
        if (controller.signal.aborted || disposed) return;
        liveNode.textContent = "polling (the live timeline ended)";
      })
      .finally(() => {
        if (follow === controller) follow = null;
      });
  }

  function stopFollow(): void {
    follow?.abort();
    follow = null;
    liveNode.textContent = "";
  }

  async function confirmOrder(order: Order): Promise<void> {
    try {
      setStatus(`Confirming ${order.names[0]}…`);
      await call(context, "/order/confirm", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ order: order.id }),
      });
      setStatus("The CA is validating the DNS record.");
      await load();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function renewOrder(order: Order): Promise<void> {
    try {
      setStatus(`Renewing ${order.names[0]}…`);
      const next = await call(context, "/order/renew", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ certificate: order.certificate }),
      });
      select(next.id);
      setStatus("The renewal order started.");
      await load();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  function openRevoke(order: Order): void {
    revoking = order.certificate ?? null;
    dialog.showModal();
  }

  async function revoke(): Promise<void> {
    if (!revoking) return;
    const reason = (
      dialog.querySelector("[data-revoke-reason]") as HTMLSelectElement
    ).value;
    try {
      setStatus("Revoking…");
      await call(context, "/order/revoke", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ certificate: revoking, reason }),
      });
      dialog.close();
      setStatus("The CA accepted the revocation.");
      await load();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  box.querySelector("[data-refresh]")?.addEventListener("click", () => {
    void load();
  });
  box.querySelector("[data-revoke-cancel]")?.addEventListener("click", () => {
    revoking = null;
    dialog.close();
  });
  box.querySelector("[data-revoke-confirm]")?.addEventListener("click", () => {
    void revoke();
  });

  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") void load();
  });
  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    if (next) {
      void load();
    }
  });
  // A modal hands the new order id through `openView` params.
  const initial = context.params as { order?: string } | undefined;
  void load().then(() => {
    if (!disposed && initial?.order) select(initial.order);
  });

  poll = window.setInterval(() => {
    if (visible && !disposed) void load();
  }, 5_000);

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    stopFollow();
    if (poll !== null) window.clearInterval(poll);
    root.replaceChildren();
  };
}
