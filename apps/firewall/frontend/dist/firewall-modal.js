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

// src/modal.ts
var STYLES = `
  .fw-modal { display: flex; flex-direction: column; gap: 14px; }
  .fw-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .fw-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .fw-modal form { display: grid; gap: 10px; }
  .fw-modal .row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .fw-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .fw-modal select, .fw-modal input {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12.5px;
  }
  .fw-modal select:focus, .fw-modal input:focus { outline: 2px solid #7aa2ff; outline-offset: 1px; }
  .fw-modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .fw-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fw-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .fw-modal button:hover:not(:disabled) { border-color: #4a5c82; color: #fff; }
  .fw-modal button:disabled { opacity: .5; cursor: default; }
  .fw-modal-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .fw-modal-status.bad { color: #ffb3ba; }
  .fw-modal-warning { padding: 8px 10px; border-radius: 7px; border: 1px solid #5a4a2a; background: #221c10; color: #f3c969; font-size: 12px; }
  @media (max-width: 520px) { .fw-modal .row { grid-template-columns: 1fr; } }
`;
function mount(root, context) {
  const params = context.params ?? {};
  const editing = params.rule;
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);
  const box = document.createElement("div");
  box.className = "fw-modal";
  box.innerHTML = `
    <div>
      <h3>${editing ? "Edit rule" : "Add rule"}</h3>
      <p>The rule lives in the managed table inet anthracite.</p>
    </div>
    <div class="fw-modal-warning" data-warning hidden></div>
    <form>
      <div class="row">
        <label>Chain
          <select data-chain aria-label="Chain">
            <option value="input">input</option>
            <option value="output">output</option>
          </select>
        </label>
        <label>Action
          <select data-action aria-label="Action">
            <option value="allow">allow</option>
            <option value="deny">deny</option>
          </select>
        </label>
      </div>
      <div class="row">
        <label>Protocol
          <select data-protocol aria-label="Protocol">
            <option value="tcp">tcp</option>
            <option value="udp">udp</option>
          </select>
        </label>
        <label>Port
          <input type="number" min="1" max="65535" data-port aria-label="Port" />
        </label>
      </div>
      <label>Comment (optional)
        <input data-comment aria-label="Comment" autocomplete="off" />
      </label>
      <div class="fw-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>${editing ? "Update rule" : "Add rule"}</button>
      </div>
    </form>
    <p class="fw-modal-status" role="status"></p>
  `;
  root.append(box);
  const form = box.querySelector("form");
  const warning = box.querySelector("[data-warning]");
  const chain = box.querySelector("[data-chain]");
  const action = box.querySelector("[data-action]");
  const protocol = box.querySelector("[data-protocol]");
  const port = box.querySelector("[data-port]");
  const comment = box.querySelector("[data-comment]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = box.querySelector(".fw-modal-status");
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "fw-modal-status bad" : "fw-modal-status";
  };
  if (editing) {
    chain.value = editing.chain;
    action.value = editing.action ?? "allow";
    protocol.value = editing.protocol ?? "tcp";
    port.value = String(editing.port ?? "");
    comment.value = editing.comment ?? "";
  }
  async function checkControl() {
    try {
      const snapshot = await call(context, "/snapshot");
      if (disposed)
        return;
      if (snapshot.control !== true) {
        warning.hidden = false;
        warning.textContent = "The firewall is read-only in this session.";
        submit.disabled = true;
      } else if (!snapshot.initialized) {
        warning.hidden = false;
        warning.textContent = "The managed table does not exist yet. Adding a rule creates it.";
      }
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = Number(port.value);
    if (!Number.isInteger(value) || value < 1 || value > 65535) {
      setStatus("Write a port between 1 and 65535.", true);
      return;
    }
    submit.disabled = true;
    setStatus(editing ? "Updating the rule…" : "Adding the rule…");
    try {
      await call(context, editing ? "/update" : "/rule", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          handle: editing?.handle,
          chain: chain.value,
          action: action.value,
          protocol: protocol.value,
          port: value,
          comment: comment.value.trim() || null
        })
      });
      if (!disposed)
        context.close();
    } catch (error) {
      submit.disabled = false;
      if (!disposed)
        setStatus(String(error), true);
    }
  });
  cancel.addEventListener("click", () => context.close());
  checkControl();
  (editing ? port : chain).focus();
  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
export {
  mount
};
