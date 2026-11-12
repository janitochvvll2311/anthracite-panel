import { call, watchNetwork } from "./api";
import { confirmAction } from "./confirm";
import { cell, count, reason } from "./format";
import { styles } from "./styles";
import type { FrameContext, Interface, Route, Snapshot } from "./types";

// Interfaces, routes and the safe mutations the fixture supports. The view
// follows `/watch` over SSE; interval 0 means manual (Refresh only).
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
      <label class="net-interval">Interval
        <select data-interval aria-label="Watch interval">
          <option value="0">Manual (0s)</option>
          <option value="1000">1s</option>
          <option value="2000" selected>2s</option>
          <option value="5000">5s</option>
          <option value="10000">10s</option>
        </select>
      </label>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="net-grid"></div>
    <div class="net-routes" role="table" aria-label="Routes"></div>
    <p class="net-status" role="status"></p>
  `;
  root.append(box);

  const grid = box.querySelector(".net-grid") as HTMLElement;
  const routes = box.querySelector(".net-routes") as HTMLElement;
  const status = box.querySelector(".net-status") as HTMLElement;
  const interval = box.querySelector("[data-interval]") as HTMLSelectElement;
  let intervalMs = 2000;
  let controller: AbortController | undefined;
  let visible = true;
  let disposed = false;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "net-status bad" : "net-status";
  };

  function render(interfaces: Interface[], table: Route[]): void {
    const active = document.activeElement as HTMLElement | null;
    const focusKey = active?.dataset.key;
    const routesScroll = routes.scrollTop;
    grid.replaceChildren();
    for (const iface of interfaces) {
      const card = document.createElement("section");
      card.className = "net-card";
      const head = document.createElement("div");
      head.className = "net-card-head";
      const toggle = document.createElement("button");
      toggle.type = "button";
      toggle.className = "net-toggle";
      toggle.dataset.key = `iface:${iface.name}`;
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
      const familyCell = cell("net-mono", route.family);
      familyCell.dataset.label = "Family";
      const destinationCell = cell("net-mono", route.destination);
      destinationCell.dataset.label = "Destination";
      const gatewayCell = cell("net-mono", route.gateway ?? "—");
      gatewayCell.dataset.label = "Gateway";
      const deviceCell = cell("", route.device);
      deviceCell.dataset.label = "Device";
      const metricCell = cell("net-mono", String(route.metric));
      metricCell.dataset.label = "Metric";
      row.append(familyCell, destinationCell, gatewayCell, deviceCell, metricCell);
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "net-remove";
      remove.dataset.key = `route:${route.destination}:${route.device}`;
      remove.title = `Remove ${route.destination} via ${route.device}`;
      remove.textContent = "×";
      remove.addEventListener("click", () => void removeRoute(route));
      const action = document.createElement("span");
      action.append(remove);
      row.append(action);
      routes.append(row);
    }
    routes.scrollTop = routesScroll;
    if (focusKey) {
      const escaped = CSS.escape(focusKey);
      (
        grid.querySelector<HTMLElement>(`[data-key="${escaped}"]`) ??
        routes.querySelector<HTMLElement>(`[data-key="${escaped}"]`)
      )?.focus();
    }
  }

  async function load(): Promise<void> {
    setStatus("Reading the network…");
    try {
      const [interfaces, table] = await Promise.all([
        call(context, "/interfaces") as Promise<Interface[]>,
        call(context, "/routes") as Promise<Route[]>,
      ]);
      if (disposed) return;
      render(interfaces, table);
      setStatus(`Updated ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function watch(): Promise<void> {
    if (disposed || intervalMs === 0) return;
    const abort = new AbortController();
    controller = abort;
    setStatus("Watching the network…");
    try {
      await watchNetwork(context, abort.signal, intervalMs, (snapshot: Snapshot) => {
        if (disposed) return;
        render(snapshot.interfaces, snapshot.routes);
        setStatus(`Live · ${new Date().toLocaleTimeString()}`);
      });
      if (!disposed && !abort.signal.aborted) {
        setStatus("The network watch ended.", true);
      }
    } catch (error) {
      if (disposed || abort.signal.aborted) return;
      setStatus(`Could not watch the network: ${reason(error)}`, true);
      // A missed stream must not leave the view empty.
      void load();
    } finally {
      if (controller === abort) controller = undefined;
    }
  }

  function restart(): void {
    controller?.abort();
    controller = undefined;
    if (intervalMs > 0) void watch();
    else void load();
  }

  async function setUp(name: string, up: boolean): Promise<void> {
    const ok = await confirmAction({
      title: up ? `Bring up ${name}?` : `Bring down ${name}?`,
      message: up
        ? "The interface will start forwarding traffic again."
        : "Traffic on this interface will stop until it is brought back up.",
      confirmLabel: up ? "Bring up" : "Bring down",
      danger: !up,
    });
    if (!ok) return;
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

  async function removeRoute(route: Route): Promise<void> {
    const ok = await confirmAction({
      title: `Remove ${route.destination}?`,
      message: `Traffic routed through ${route.device} will stop using this entry.`,
      confirmLabel: "Remove route",
      danger: true,
    });
    if (!ok) return;
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

  interval.addEventListener("change", () => {
    intervalMs = Number(interval.value) || 0;
    restart();
  });
  box.querySelector("[data-refresh]")!.addEventListener("click", restart);
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") void load();
  });
  const offVisibility = context.onVisibility?.((next) => {
    if (next === visible) return;
    visible = next;
    if (!visible) {
      controller?.abort();
      controller = undefined;
    } else if (intervalMs > 0) {
      void watch();
    }
  });
  void (intervalMs > 0 ? watch() : load());

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    controller?.abort();
    controller = undefined;
    root.replaceChildren();
  };
}
