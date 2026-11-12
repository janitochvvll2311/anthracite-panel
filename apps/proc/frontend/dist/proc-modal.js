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
  .proc-modal { display: flex; flex-direction: column; gap: 14px; }
  .proc-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .proc-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .proc-modal form { display: grid; gap: 10px; }
  .proc-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .proc-modal input {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12.5px ui-monospace, monospace;
  }
  .proc-modal input:focus { outline: 2px solid #7aa2ff; outline-offset: 1px; }
  .proc-modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .proc-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .proc-modal button:hover:not(:disabled) { border-color: #4a5c82; color: #fff; }
  .proc-modal button:disabled { opacity: .5; cursor: default; }
  .proc-modal-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .proc-modal-status.bad { color: #ffb3ba; }
`;
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);
  const box = document.createElement("div");
  box.className = "proc-modal";
  box.innerHTML = `
    <div>
      <h3>Spawn a process</h3>
      <p>The command runs detached from the process service. Shell syntax is not interpreted.</p>
    </div>
    <form>
      <label>Command
        <input data-command placeholder="sleep 30" aria-label="Detached command" autocomplete="off" />
      </label>
      <div class="proc-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Spawn</button>
      </div>
    </form>
    <p class="proc-modal-status" role="status"></p>
  `;
  root.append(box);
  const form = box.querySelector("form");
  const command = box.querySelector("[data-command]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = box.querySelector(".proc-modal-status");
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "proc-modal-status bad" : "proc-modal-status";
  };
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = command.value.trim();
    if (!value) {
      setStatus("Write a command first, for example `sleep 30`.", true);
      return;
    }
    const argv = value.split(/\s+/);
    submit.disabled = true;
    setStatus("Spawning…");
    try {
      await call(context, "/spawn", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ argv })
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
  command.focus();
  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
export {
  mount
};
