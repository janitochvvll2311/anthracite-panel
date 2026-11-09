// The demo Network view: interfaces and routes from the net service.

const styles = `
  .net { display: flex; flex-direction: column; gap: 14px; min-height: 360px; }
  .net-bar { display: flex; align-items: center; gap: 10px; }
  .net-bar .grow { flex: 1; }
  .net-bar button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .net-bar button:hover { border-color: #3a4560; color: #fff; }
  .net-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 10px; }
  .net-card {
    border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 11px 12px;
    display: grid; gap: 7px;
  }
  .net-card-head { display: flex; align-items: center; gap: 8px; }
  .net-name { font-weight: 600; color: #e8ebf2; }
  .net-state { margin-left: auto; font-size: 10px; text-transform: uppercase; letter-spacing: .08em; }
  .net-state.up { color: #52d1a4; }
  .net-state.down { color: #667087; }
  .net-meta { color: #667087; font: 11px ui-monospace, monospace; display: flex; gap: 12px; flex-wrap: wrap; }
  .net-addrs { display: flex; gap: 5px; flex-wrap: wrap; }
  .net-chip {
    padding: 2px 7px; border-radius: 999px; border: 1px solid #2a3142; color: #b9c3d6;
    font: 11px ui-monospace, monospace;
  }
  .net-chip.ipv6 { border-color: #9d7bff44; color: #cbb8ff; }
  .net-counters { display: flex; gap: 14px; color: #8f98ab; font-size: 11px; }
  .net-counters b { color: #c8d0df; font-weight: 500; }
  .net-routes { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .net-route {
    display: grid; grid-template-columns: 60px minmax(150px, 1fr) minmax(120px, 1fr) 100px 70px;
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px;
  }
  .net-route:last-child { border-bottom: 0; }
  .net-route.head {
    position: sticky; top: 0; background: #11151e; color: #667087; text-transform: uppercase;
    letter-spacing: .06em; font-size: 10px;
  }
  .net-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .net-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .net-status.bad { color: #ffb3ba; }
`;

async function call(context, route) {
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}${route}`,
    { headers: { accept: "application/json" } },
  );
  const body = await response.text();
  let value = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message =
      value && typeof value === "object" && value.error
        ? `${value.error.code}: ${value.error.message}`
        : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}

function count(bytes) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} B`;
}

function cell(className, text) {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
}

export function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "net";
  box.innerHTML = `
    <div class="net-bar">
      <span class="net-mono">interfaces and routes</span>
      <span class="grow"></span>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="net-grid"></div>
    <div class="net-routes" role="table" aria-label="Routes"></div>
    <p class="net-status" role="status"></p>
  `;
  root.append(box);

  const grid = box.querySelector(".net-grid");
  const routes = box.querySelector(".net-routes");
  const status = box.querySelector(".net-status");
  let disposed = false;

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "net-status bad" : "net-status";
  };

  async function load() {
    setStatus("Reading the network…");
    try {
      const [interfaces, table] = await Promise.all([
        call(context, "/interfaces"),
        call(context, "/routes"),
      ]);
      if (disposed) return;
      grid.replaceChildren();
      for (const iface of interfaces) {
        const card = document.createElement("section");
        card.className = "net-card";
        const head = document.createElement("div");
        head.className = "net-card-head";
        head.append(
          cell("net-name", iface.name),
          cell(`net-state ${iface.up ? "up" : "down"}`, iface.up ? "up" : "down"),
        );
        const meta = document.createElement("div");
        meta.className = "net-meta";
        meta.append(cell("", `mac ${iface.mac}`), cell("", `mtu ${iface.mtu}`));
        const addrs = document.createElement("div");
        addrs.className = "net-addrs";
        for (const address of iface.addresses) {
          addrs.append(
            cell(
              `net-chip ${address.family}`,
              `${address.address}/${address.prefix}`,
            ),
          );
        }
        const counters = document.createElement("div");
        counters.className = "net-counters";
        counters.innerHTML = `<span>rx <b>${count(iface["rx-bytes"])}</b></span><span>tx <b>${count(iface["tx-bytes"])}</b></span>`;
        card.append(head, meta, addrs, counters);
        grid.append(card);
      }

      routes.replaceChildren();
      const head = document.createElement("div");
      head.className = "net-route head";
      for (const label of ["FAMILY", "DESTINATION", "GATEWAY", "DEVICE", "METRIC"]) {
        head.append(cell("", label));
      }
      routes.append(head);
      for (const route of table) {
        const row = document.createElement("div");
        row.className = "net-route";
        row.append(
          cell("net-mono", route.family),
          cell("net-mono", route.destination),
          cell("net-mono", route.gateway ?? "—"),
          cell("", route.device),
          cell("net-mono", String(route.metric)),
        );
        routes.append(row);
      }
      setStatus(`${interfaces.length} interfaces · ${table.length} routes`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  void load();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
