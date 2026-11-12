import { call } from "./api";
import { confirmAction } from "./confirm";
import { styles } from "./styles";
import type { FrameContext } from "./types";

// The firewall view: one managed nftables table with two base chains.

export function mount(root: HTMLElement, context: FrameContext) {
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

  const policyRoot = box.querySelector("[data-policy]") as HTMLElement;
  const chainsRoot = box.querySelector("[data-chains]") as HTMLElement;
  const rulesRoot = box.querySelector("[data-rules]") as HTMLElement;
  const status = box.querySelector(".fw-status") as HTMLElement;
  const form = box.querySelector("[data-form]") as HTMLFormElement;
  const handle = box.querySelector("[data-handle]") as HTMLInputElement;
  const chain = box.querySelector("[data-chain]") as HTMLSelectElement;
  const action = box.querySelector("[data-action]") as HTMLSelectElement;
  const protocol = box.querySelector("[data-protocol]") as HTMLSelectElement;
  const port = box.querySelector("[data-port]") as HTMLInputElement;
  const comment = box.querySelector("[data-comment]") as HTMLInputElement;
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  const cancel = box.querySelector("[data-cancel]") as HTMLButtonElement;
  let control = false;
  let disposed = false;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "fw-status bad" : "fw-status";
  };

  const badge = (label: string, className = ""): HTMLElement => {
    const node = document.createElement("span");
    node.className = `fw-badge ${className}`.trim();
    node.textContent = label;
    return node;
  };

  const button = (label: string, className = ""): HTMLButtonElement => {
    const node = document.createElement("button");
    node.type = "button";
    node.className = className;
    node.textContent = label;
    return node;
  };

  function resetForm(): void {
    form.reset();
    handle.value = "";
    protocol.value = "tcp";
    action.value = "allow";
    submit.textContent = "Add rule";
    cancel.hidden = true;
  }

  function editRule(rule: any): void {
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

  function render(snapshot: any): void {
    control = snapshot.control === true;
    policyRoot.replaceChildren(
      badge(snapshot.initialized ? "table ready" : "table missing", snapshot.initialized ? "on" : "off"),
      badge(control ? "control on" : "read-only", control ? "on" : "off"),
    );
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
      const target = custom
        ? "custom"
        : `${rule.protocol}:${rule.port}`;
      const cells = [
        String(rule.handle),
        rule.chain,
        custom ? "custom" : rule.action,
        target,
        rule.comment ?? "—",
      ];
      for (const value of cells) {
        const cell = document.createElement("span");
        cell.className = value === String(rule.handle) || value === target ? "fw-mono" : "";
        cell.textContent = value;
        row.append(cell);
      }
      if (custom) row.querySelectorAll("span")[2]?.replaceChildren(badge("custom", "custom"));
      const actions = document.createElement("span");
      actions.className = "fw-actions";
      const edit = button("Edit", "fw-btn");
      edit.disabled = !control || custom;
      edit.title = custom ? "custom rules can only be deleted" : `Edit rule ${rule.handle}`;
      edit.addEventListener("click", () => editRule(rule));
      const remove = button("Delete", "fw-btn danger");
      remove.disabled = !control;
      remove.title = remove.disabled ? "read-only" : `Delete rule ${rule.handle}`;
      remove.addEventListener("click", () => void deleteRule(rule));
      actions.append(edit, remove);
      row.append(actions);
      rulesRoot.append(row);
    }

    const inputs = form.querySelectorAll("input:not([type=hidden]), select, button");
    for (const input of inputs) {
      (input as HTMLInputElement).disabled = !control;
    }
    if (!snapshot.initialized) {
      setStatus("The managed table does not exist yet. Adding a rule creates it.");
    } else {
      setStatus(`${snapshot.rules?.length ?? 0} rules`);
    }
  }

  async function load(): Promise<void> {
    setStatus("Reading the firewall…");
    try {
      const snapshot = await call(context, "/snapshot");
      if (disposed) return;
      render(snapshot);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function flushChain(chainName: string): Promise<void> {
    const ok = await confirmAction({
      title: `Flush ${chainName}?`,
      message: "Every rule in this chain will be removed. The policies stay.",
      confirmLabel: "Flush",
      danger: true,
    });
    if (!ok) return;
    setStatus(`Flushing ${chainName}…`);
    try {
      await call(context, "/flush", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ chain: chainName }),
      });
      await load();
      setStatus(`Flushed ${chainName}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function setPolicy(chainName: string, next: string): Promise<void> {
    const message =
      next === "deny"
        ? "Packets without an accept rule will be dropped, including management traffic. Keep an out-of-band console ready."
        : "The chain will accept packets that no rule drops.";
    const ok = await confirmAction({
      title: `Set ${chainName} policy to ${next}?`,
      message,
      confirmLabel: `Set ${next}`,
      danger: next === "deny",
    });
    if (!ok) return;
    setStatus(`Setting ${chainName} policy to ${next}…`);
    try {
      await call(context, "/policy", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ chain: chainName, policy: next }),
      });
      await load();
      setStatus(`${chainName} policy is ${next}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function deleteRule(rule: any): Promise<void> {
    const ok = await confirmAction({
      title: `Delete rule ${rule.handle}?`,
      message: "The rule will be removed from the managed table.",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    setStatus(`Deleting rule ${rule.handle}…`);
    try {
      await call(context, `/rule?handle=${encodeURIComponent(rule.handle)}`, {
        method: "DELETE",
      });
      await load();
      setStatus(`Deleted rule ${rule.handle}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

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
      comment: comment.value.trim() || null,
    };
    setStatus(editing ? "Updating the rule…" : "Adding the rule…");
    try {
      await call(context, editing ? "/update" : "/rule", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      resetForm();
      await load();
      setStatus(editing ? "Rule updated" : "Rule added");
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  });

  cancel.addEventListener("click", resetForm);
  box.querySelector("[data-refresh]")!.addEventListener("click", () => void load());
  let visible = true;
  const offVisibility = context.onVisibility?.((next) => {
    if (next === visible) return;
    visible = next;
    if (!next || disposed) return;
    void load();
  });
  void load();

  return () => {
    disposed = true;
    offVisibility?.();
    root.replaceChildren();
  };
}
