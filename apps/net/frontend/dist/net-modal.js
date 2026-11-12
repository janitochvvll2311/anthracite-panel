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
function addRoute(context, route) {
  return call(context, "/route", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(route)
  });
}

// src/modal.ts
var STYLES = `
  .net-modal { display: flex; flex-direction: column; gap: 14px; }
  .net-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .net-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .net-modal form { display: grid; gap: 10px; }
  .net-modal .row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .net-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .net-modal select, .net-modal input {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12.5px;
  }
  .net-modal select:focus, .net-modal input:focus { outline: 2px solid #7aa2ff; outline-offset: 1px; }
  .net-modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .net-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .net-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .net-modal button:hover:not(:disabled) { border-color: #4a5c82; color: #fff; }
  .net-modal button:disabled { opacity: .5; cursor: default; }
  .net-modal-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .net-modal-status.bad { color: #ffb3ba; }
  @media (max-width: 520px) { .net-modal .row { grid-template-columns: 1fr; } }
`;
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);
  const box = document.createElement("div");
  box.className = "net-modal";
  box.innerHTML = `
    <div>
      <h3>Add route</h3>
      <p>A wrong route can interrupt connectivity. Check the gateway and device.</p>
    </div>
    <form>
      <div class="row">
        <label>Family
          <select data-family aria-label="Family">
            <option value="ipv4">ipv4</option>
            <option value="ipv6">ipv6</option>
          </select>
        </label>
        <label>Device <select data-device aria-label="Device"></select></label>
      </div>
      <label>Destination
        <input data-destination placeholder="10.1.0.0/24" aria-label="Destination" autocomplete="off" />
      </label>
      <div class="row">
        <label>Gateway (optional)
          <input data-gateway placeholder="192.168.1.1" aria-label="Gateway" autocomplete="off" />
        </label>
        <label>Metric
          <input data-metric type="number" min="0" value="100" aria-label="Metric" />
        </label>
      </div>
      <div class="net-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Add route</button>
      </div>
    </form>
    <p class="net-modal-status" role="status"></p>
  `;
  root.append(box);
  const form = box.querySelector("form");
  const family = box.querySelector("[data-family]");
  const destination = box.querySelector("[data-destination]");
  const gateway = box.querySelector("[data-gateway]");
  const device = box.querySelector("[data-device]");
  const metric = box.querySelector("[data-metric]");
  const submit = box.querySelector("[data-submit]");
  submit.disabled = true;
  const cancel = box.querySelector("[data-cancel]");
  const status = box.querySelector(".net-modal-status");
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "net-modal-status bad" : "net-modal-status";
  };
  async function load() {
    setStatus("Reading interfaces…");
    try {
      const interfaces = await call(context, "/interfaces");
      if (disposed)
        return;
      device.replaceChildren();
      for (const iface of interfaces) {
        const option = document.createElement("option");
        option.value = iface.name;
        option.textContent = `${iface.name} · ${iface.up ? "up" : "down"}`;
        device.append(option);
      }
      submit.disabled = device.options.length === 0;
      setStatus(device.options.length === 0 ? "There are no interfaces to route through." : "The route is added to the live table.", device.options.length === 0);
      destination.focus();
    } catch (error) {
      submit.disabled = true;
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const path = destination.value.trim();
    if (!path) {
      setStatus("Write a destination first.", true);
      return;
    }
    submit.disabled = true;
    setStatus("Adding the route…");
    try {
      await addRoute(context, {
        family: family.value,
        destination: path,
        gateway: gateway.value.trim() || null,
        device: device.value,
        metric: Number(metric.value) || 100
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
  load();
  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
export {
  mount
};
