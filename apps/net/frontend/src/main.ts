import { call } from "./api";
import { cell, count, reason } from "./format";
import { styles } from "./styles";
import type { FrameContext } from "./types";

// The demo Network view: interfaces, routes and the safe mutations the fixture
// supports (bring an interface up/down, add or remove a static route).

export function mount(root: HTMLElement, context: FrameContext) {
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
    <form class="net-add">
      <select data-family aria-label="Family">
        <option value="ipv4">ipv4</option>
        <option value="ipv6">ipv6</option>
      </select>
      <input data-destination placeholder="destination, e.g. 10.1.0.0/24" aria-label="Destination" autocomplete="off" />
      <input data-gateway placeholder="gateway (optional)" aria-label="Gateway" autocomplete="off" />
      <select data-device aria-label="Device"></select>
      <input data-metric type="number" min="0" value="100" aria-label="Metric" />
      <button type="submit">Add route</button>
    </form>
    <div class="net-routes" role="table" aria-label="Routes"></div>
    <p class="net-status" role="status"></p>
  `;
  root.append(box);

  const grid = box.querySelector(".net-grid") as any;
  const routes = box.querySelector(".net-routes") as any;
  const status = box.querySelector(".net-status") as any;
  const form = box.querySelector(".net-add") as any;
  const family = box.querySelector("[data-family]") as any;
  const destination = box.querySelector("[data-destination]") as any;
  const gateway = box.querySelector("[data-gateway]") as any;
  const device = box.querySelector("[data-device]") as any;
  const metric = box.querySelector("[data-metric]") as any;
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
        const toggle = document.createElement("button");
        toggle.type = "button";
        toggle.className = "net-toggle";
        toggle.textContent = iface.up ? "Down" : "Up";
        toggle.title = `${iface.up ? "Bring down" : "Bring up"} ${iface.name}`;
        toggle.addEventListener("click", () => void setUp(iface.name, !iface.up));
        head.append(
          cell("net-name", iface.name),
          cell(`net-state ${iface.up ? "up" : "down"}`, iface.up ? "up" : "down"),
          toggle,
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

      const selected = device.value;
      device.replaceChildren();
      for (const iface of interfaces) {
        const option = document.createElement("option");
        option.value = iface.name;
        option.textContent = iface.name;
        device.append(option);
      }
      if (interfaces.some((iface) => iface.name === selected)) device.value = selected;

      routes.replaceChildren();
      const head = document.createElement("div");
      head.className = "net-route head";
      for (const label of ["FAMILY", "DESTINATION", "GATEWAY", "DEVICE", "METRIC", ""]) {
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
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "net-remove";
        remove.title = `Remove ${route.destination} via ${route.device}`;
        remove.textContent = "×";
        remove.addEventListener("click", () => void removeRoute(route));
        const action = document.createElement("span");
        action.append(remove);
        row.append(action);
        routes.append(row);
      }
      setStatus(
        `${interfaces.length} interfaces · ${table.length} routes · updated ${new Date().toLocaleTimeString()}`,
      );
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function setUp(name, up) {
    setStatus(`${up ? "Bringing up" : "Bringing down"} ${name}…`);
    try {
      await call(context, "/up", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name, up }),
      });
      await load();
      setStatus(`${name} is ${up ? "up" : "down"}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function removeRoute(route) {
    setStatus(`Removing ${route.destination}…`);
    try {
      await call(
        context,
        `/route?destination=${encodeURIComponent(route.destination)}&device=${encodeURIComponent(route.device)}`,
        { method: "DELETE" },
      );
      await load();
      setStatus(`Removed ${route.destination} via ${route.device}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const path = destination.value.trim();
    if (!path) {
      setStatus("Write a destination first.", true);
      return;
    }
    setStatus("Adding the route…");
    try {
      await call(context, "/route", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          family: family.value,
          destination: path,
          gateway: gateway.value.trim() || null,
          device: device.value,
          metric: Number(metric.value) || 100,
        }),
      });
      destination.value = "";
      gateway.value = "";
      await load();
      setStatus(`Added ${path} via ${device.value}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  });

  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  void load();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
