import { empty, meter, query, rows, span } from "./dom";
import { formatBytes, formatDuration, percent } from "./format";
import type {
  Cpu,
  Disk,
  HostInfo,
  Memory,
  NetworkInterface,
} from "./types";

export function renderHost(root: HTMLElement, info: HostInfo): void {
  renderSystem(root, info);
  renderCpu(root, info.cpu);
  renderMemory(root, info.memory);
  renderDisks(root, info.disks);
  renderNetworks(root, info.networks);
}

function renderSystem(root: HTMLElement, info: HostInfo): void {
  const target = query<HTMLElement>(root, "[data-system]");
  if (!target) return;
  rows(target, [
    ["Hostname", info.hostname],
    ["OS", info.os],
    ["Kernel", info.kernel],
    ["Architecture", info.arch],
    ["Uptime", formatDuration(info["uptime-seconds"])],
  ]);
}

function renderCpu(root: HTMLElement, cpu: Cpu): void {
  const target = query<HTMLElement>(root, "[data-cpu]");
  if (!target) return;
  target.replaceChildren();
  const details = document.createElement("dl");
  rows(details, [
    ["Model", cpu.model],
    ["Cores", String(cpu.cores)],
    [
      "Load",
      `${Number(cpu["load-1"]).toFixed(2)} / ${Number(cpu["load-5"]).toFixed(2)} / ${Number(cpu["load-15"]).toFixed(2)}`,
    ],
  ]);
  const label = document.createElement("div");
  label.className = "home-bar-label";
  label.append(
    span("Usage"),
    span(`${Number(cpu["usage-percent"]).toFixed(1)}%`),
  );
  target.append(details, label, meter(cpu["usage-percent"], 100, true));
}

function renderMemory(root: HTMLElement, memory: Memory): void {
  const target = query<HTMLElement>(root, "[data-memory]");
  if (!target) return;
  target.replaceChildren();
  const label = document.createElement("div");
  label.className = "home-bar-label";
  label.append(
    span(`${formatBytes(memory.used)} / ${formatBytes(memory.total)}`),
    span(`${percent(memory.used, memory.total).toFixed(0)}%`),
  );
  const details = document.createElement("dl");
  rows(details, [
    ["Available", formatBytes(memory.available)],
    [
      "Swap",
      memory["swap-total"] > 0
        ? `${formatBytes(memory["swap-used"])} / ${formatBytes(memory["swap-total"])}`
        : "not configured",
    ],
  ]);
  target.append(label, meter(memory.used, memory.total), details);
}

function renderDisks(root: HTMLElement, disks: Disk[]): void {
  const target = query<HTMLElement>(root, "[data-disks]");
  if (!target) return;
  target.replaceChildren();
  if (disks.length === 0) {
    target.append(empty("No disks detected."));
    return;
  }
  for (const disk of disks) {
    const row = document.createElement("div");
    row.className = "home-disk";
    const head = document.createElement("div");
    head.className = "home-disk-head";
    head.append(
      span(disk.mount, "home-disk-name"),
      span(`${disk["file-system"]} · ${disk.name}`, "home-disk-meta"),
    );
    const label = document.createElement("div");
    label.className = "home-bar-label";
    label.append(
      span(`${formatBytes(disk.used)} of ${formatBytes(disk.total)}`),
      span(`${formatBytes(disk.available)} free`),
    );
    row.append(head, label, meter(disk.used, disk.total));
    target.append(row);
  }
}

function renderNetworks(
  root: HTMLElement,
  networks: NetworkInterface[],
): void {
  const target = query<HTMLElement>(root, "[data-network]");
  if (!target) return;
  target.replaceChildren();
  if (networks.length === 0) {
    target.append(empty("No network interfaces detected."));
    return;
  }
  for (const network of networks) {
    const row = document.createElement("div");
    row.className = "home-net";
    const head = document.createElement("div");
    head.className = "home-net-head";
    const up = ["up", "unknown"].includes(
      String(network.state).toLowerCase(),
    );
    head.append(
      span(network.name, "home-net-name"),
      span(
        network.state || "unknown",
        up ? "home-net-state" : "home-net-state down",
      ),
      span(network.mac ?? "no MAC", "home-net-meta"),
    );
    const addresses = document.createElement("div");
    addresses.className = "home-net-addr";
    addresses.textContent =
      network.addresses.length > 0
        ? network.addresses.join(" · ")
        : "No addresses";
    const io = document.createElement("div");
    io.className = "home-net-io";
    io.textContent =
      `↓ ${formatBytes(network.received)} / ${network["packets-received"]} pkts · ` +
      `↑ ${formatBytes(network.transmitted)} / ${network["packets-transmitted"]} pkts · ` +
      `${network.errors} errors`;
    row.append(head, addresses, io);
    target.append(row);
  }
}
