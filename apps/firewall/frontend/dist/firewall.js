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

// src/styles.ts
var styles = `
  .fw { display: flex; flex-direction: column; gap: 14px; min-height: 360px; }
  .fw-bar { display: flex; align-items: center; gap: 10px; }
  .fw-bar .grow { flex: 1; }
  .fw-bar button, .fw button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fw-bar button:hover, .fw button:hover { border-color: #3a4560; color: #fff; }
  .fw-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .fw-policy, .fw-chains { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }
  .fw-card { border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 10px 12px; min-width: 180px; }
  .fw-card-head { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  .fw-card-head strong { color: #e8ebf2; font-size: 13px; }
  .fw-badge { padding: 1px 6px; border-radius: 999px; border: 1px solid #2a3142; font-size: 9.5px; text-transform: uppercase; letter-spacing: .07em; color: #8f98ab; }
  .fw-badge.allow { border-color: #6fcf9744; color: #9fe0b8; }
  .fw-badge.deny { border-color: #ff7d8a44; color: #ffb3ba; }
  .fw-badge.custom { border-color: #d8b45a44; color: #e5ca83; }
  .fw-badge.on { border-color: #6fcf9744; color: #9fe0b8; }
  .fw-badge.off { border-color: #ff7d8a44; color: #ffb3ba; }
  .fw-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .fw-row { display: grid; grid-template-columns: 50px 70px 70px 90px minmax(120px, 1fr) minmax(120px, auto); gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center; }
  .fw-row:last-child { border-bottom: 0; }
  .fw-row.head { background: #11151e; color: #667087; text-transform: uppercase; letter-spacing: .06em; font-size: 10px; }
  .fw-actions { display: flex; gap: 4px; flex-wrap: wrap; }
  .fw-btn { padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24; color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer; }
  .fw-btn.danger:hover { border-color: #ff7d8a66; color: #ffb3ba; }
  .fw-form { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .fw-form select, .fw-form input {
    padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .fw-form input[data-port] { width: 76px; }
  .fw-form input[data-comment] { flex: 1; min-width: 130px; }
  .fw-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .fw-status.bad { color: #ffb3ba; }
  .fw-warning { color: #e5ca83; font-size: 12px; }
  @media (max-width: 860px) {
    .fw-bar { flex-wrap: wrap; }
    .fw-policy, .fw-chains { align-items: stretch; }
    .fw-card { width: 100%; min-width: 0; }
    .fw-table { border: 0; background: transparent; overflow: visible; }
    .fw-row.head { display: none; }
    .fw-row:not(.head) {
      display: grid; grid-template-columns: minmax(0, 1fr); gap: 4px;
      padding: 10px 12px; margin-bottom: 8px; border: 1px solid #202632;
      border-radius: 9px; background: #0d1016;
    }
    .fw-row:not(.head) > span:not(.fw-actions) {
      display: flex; justify-content: space-between; gap: 12px; font-size: 11.5px;
    }
    .fw-row:not(.head) > span[data-label]::before {
      content: attr(data-label); color: #667087; font-size: 9.5px;
      text-transform: uppercase; letter-spacing: .06em;
    }
    .fw-actions { margin-top: 6px; padding-top: 6px; border-top: 1px solid #171c26; }
    .fw-form { align-items: stretch; }
    .fw-form input, .fw-form select, .fw-form button { width: 100%; }
    .fw-form input[data-port], .fw-form input[data-comment] { width: 100%; }
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
  box.className = "fw";
  box.innerHTML = `
    <div class="fw-bar">
      <span class="fw-mono">managed table inet anthracite</span>
      <span class="grow"></span>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="fw-policy" data-policy></div>
    <div class="fw-chains" data-chains></div>
    <div class="fw-table" data-rules></div>
    <form class="fw-form" data-form>
      <input type="hidden" data-handle />
      <select data-chain aria-label="Chain">
        <option value="input">input</option>
        <option value="output">output</option>
      </select>
      <select data-action aria-label="Action">
        <option value="allow">allow</option>
        <option value="deny">deny</option>
      </select>
      <select data-protocol aria-label="Protocol">
        <option value="tcp">tcp</option>
        <option value="udp">udp</option>
      </select>
      <input type="number" min="1" max="65535" data-port placeholder="port" aria-label="Port" />
      <input data-comment placeholder="comment (optional)" aria-label="Comment" autocomplete="off" />
      <button type="submit" data-submit>Add rule</button>
      <button type="button" data-cancel hidden>Cancel</button>
    </form>
    <p class="fw-status" role="status"></p>
  `;
  root.append(box);
  const policyRoot = box.querySelector("[data-policy]");
  const chainsRoot = box.querySelector("[data-chains]");
  const rulesRoot = box.querySelector("[data-rules]");
  const status = box.querySelector(".fw-status");
  const form = box.querySelector("[data-form]");
  const handle = box.querySelector("[data-handle]");
  const chain = box.querySelector("[data-chain]");
  const action = box.querySelector("[data-action]");
  const protocol = box.querySelector("[data-protocol]");
  const port = box.querySelector("[data-port]");
  const comment = box.querySelector("[data-comment]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const usePanelForms = context.panel?.slots?.includes("panel.fab") === true && context.panel?.slots?.includes("panel.modal") === true;
  if (usePanelForms)
    form.remove();
  let control = false;
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "fw-status bad" : "fw-status";
  };
  const badge = (label, className = "") => {
    const node = document.createElement("span");
    node.className = `fw-badge ${className}`.trim();
    node.textContent = label;
    return node;
  };
  const button = (label, className = "") => {
    const node = document.createElement("button");
    node.type = "button";
    node.className = className;
    node.textContent = label;
    return node;
  };
  function resetForm() {
    form.reset();
    handle.value = "";
    protocol.value = "tcp";
    action.value = "allow";
    submit.textContent = "Add rule";
    cancel.hidden = true;
  }
  function editRule(rule) {
    handle.value = String(rule.handle);
    chain.value = rule.chain;
    action.value = rule.action ?? "allow";
    protocol.value = rule.protocol ?? "tcp";
    port.value = String(rule.port ?? "");
    comment.value = rule.comment ?? "";
    submit.textContent = "Update rule";
    cancel.hidden = false;
    port.focus();
  }
  function render(snapshot) {
    control = snapshot.control === true;
    policyRoot.replaceChildren(badge(snapshot.initialized ? "table ready" : "table missing", snapshot.initialized ? "on" : "off"), badge(control ? "control on" : "read-only", control ? "on" : "off"));
    chainsRoot.replaceChildren();
    for (const state of snapshot.chains ?? []) {
      const card = document.createElement("section");
      card.className = "fw-card";
      const head = document.createElement("div");
      head.className = "fw-card-head";
      head.replaceChildren(document.createTextNode(state.chain));
      head.append(badge(state.initialized ? "ready" : "missing", state.initialized ? "on" : "off"));
      head.append(badge(`policy ${state.policy}`, state.policy === "deny" ? "deny" : "allow"));
      card.append(head);
      const tools = document.createElement("div");
      tools.className = "fw-actions";
      const flush = button("Flush", "fw-btn danger");
      flush.disabled = !control || !state.initialized;
      flush.title = flush.disabled ? "read-only or chain missing" : `Remove all rules from ${state.chain}`;
      flush.addEventListener("click", () => void flushChain(state.chain));
      const next = state.policy === "deny" ? "allow" : "deny";
      const policyButton = button(`Set ${next}`, "fw-btn");
      policyButton.disabled = !control;
      policyButton.title = policyButton.disabled ? "read-only" : `Set ${state.chain} default policy to ${next}`;
      policyButton.addEventListener("click", () => void setPolicy(state.chain, next));
      tools.append(flush, policyButton);
      card.append(tools);
      chainsRoot.append(card);
    }
    rulesRoot.replaceChildren();
    const head = document.createElement("div");
    head.className = "fw-row head";
    for (const label of ["HANDLE", "CHAIN", "ACTION", "TARGET", "COMMENT", "ACTIONS"]) {
      const cell = document.createElement("span");
      cell.textContent = label;
      head.append(cell);
    }
    rulesRoot.append(head);
    for (const rule of snapshot.rules ?? []) {
      const row = document.createElement("div");
      row.className = "fw-row";
      const custom = rule.action === null || rule.action === undefined;
      const target = custom ? "custom" : `${rule.protocol}:${rule.port}`;
      const cells = [
        String(rule.handle),
        rule.chain,
        custom ? "custom" : rule.action,
        target,
        rule.comment ?? "—"
      ];
      const labels = ["Handle", "Chain", "Action", "Target", "Comment"];
      cells.forEach((value, index) => {
        const cell = document.createElement("span");
        cell.className = value === String(rule.handle) || value === target ? "fw-mono" : "";
        cell.dataset.label = labels[index];
        cell.textContent = value;
        row.append(cell);
      });
      if (custom)
        row.querySelectorAll("span")[2]?.replaceChildren(badge("custom", "custom"));
      const actions = document.createElement("span");
      actions.className = "fw-actions";
      const edit = button("Edit", "fw-btn");
      edit.disabled = !control || custom;
      edit.title = custom ? "custom rules can only be deleted" : `Edit rule ${rule.handle}`;
      edit.addEventListener("click", () => {
        if (!usePanelForms) {
          editRule(rule);
          return;
        }
        context.openModal?.({
          tag: "firewall-rule-modal",
          params: {
            rule: {
              handle: rule.handle,
              chain: rule.chain,
              action: rule.action,
              protocol: rule.protocol,
              port: rule.port,
              comment: rule.comment
            }
          }
        });
      });
      const remove = button("Delete", "fw-btn danger");
      remove.disabled = !control;
      remove.title = remove.disabled ? "read-only" : `Delete rule ${rule.handle}`;
      remove.addEventListener("click", () => void deleteRule(rule));
      actions.append(edit, remove);
      row.append(actions);
      rulesRoot.append(row);
    }
    if (!usePanelForms) {
      const inputs = form.querySelectorAll("input:not([type=hidden]), select, button");
      for (const input of inputs) {
        input.disabled = !control;
      }
    }
    if (!snapshot.initialized) {
      setStatus("The managed table does not exist yet. Adding a rule creates it.");
    } else {
      setStatus(`${snapshot.rules?.length ?? 0} rules`);
    }
  }
  async function load() {
    setStatus("Reading the firewall…");
    try {
      const snapshot = await call(context, "/snapshot");
      if (disposed)
        return;
      render(snapshot);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function flushChain(chainName) {
    const ok = await confirmAction({
      title: `Flush ${chainName}?`,
      message: "Every rule in this chain will be removed. The policies stay.",
      confirmLabel: "Flush",
      danger: true
    });
    if (!ok)
      return;
    setStatus(`Flushing ${chainName}…`);
    try {
      await call(context, "/flush", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ chain: chainName })
      });
      await load();
      setStatus(`Flushed ${chainName}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function setPolicy(chainName, next) {
    const message = next === "deny" ? "Packets without an accept rule will be dropped, including management traffic. Keep an out-of-band console ready." : "The chain will accept packets that no rule drops.";
    const ok = await confirmAction({
      title: `Set ${chainName} policy to ${next}?`,
      message,
      confirmLabel: `Set ${next}`,
      danger: next === "deny"
    });
    if (!ok)
      return;
    setStatus(`Setting ${chainName} policy to ${next}…`);
    try {
      await call(context, "/policy", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ chain: chainName, policy: next })
      });
      await load();
      setStatus(`${chainName} policy is ${next}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function deleteRule(rule) {
    const ok = await confirmAction({
      title: `Delete rule ${rule.handle}?`,
      message: "The rule will be removed from the managed table.",
      confirmLabel: "Delete",
      danger: true
    });
    if (!ok)
      return;
    setStatus(`Deleting rule ${rule.handle}…`);
    try {
      await call(context, `/rule?handle=${encodeURIComponent(rule.handle)}`, {
        method: "DELETE"
      });
      await load();
      setStatus(`Deleted rule ${rule.handle}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  if (!usePanelForms) {
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const value = Number(port.value);
      if (!Number.isInteger(value) || value < 1 || value > 65535) {
        setStatus("Write a port between 1 and 65535.", true);
        return;
      }
      const editing = handle.value !== "";
      const body = {
        handle: editing ? Number(handle.value) : undefined,
        chain: chain.value,
        action: action.value,
        protocol: protocol.value,
        port: value,
        comment: comment.value.trim() || null
      };
      setStatus(editing ? "Updating the rule…" : "Adding the rule…");
      try {
        await call(context, editing ? "/update" : "/rule", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body)
        });
        resetForm();
        await load();
        setStatus(editing ? "Rule updated" : "Rule added");
      } catch (error) {
        if (!disposed)
          setStatus(String(error), true);
      }
    });
    cancel.addEventListener("click", resetForm);
  }
  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh")
      load();
  });
  let visible = true;
  const offVisibility = context.onVisibility?.((next) => {
    if (next === visible)
      return;
    visible = next;
    if (!next || disposed)
      return;
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
