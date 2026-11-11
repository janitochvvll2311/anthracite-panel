import { call } from "./api";
import { confirmAction } from "./confirm";
import { cell, size } from "./format";
import { styles } from "./styles";
import type { FrameContext } from "./types";

// The demo Storage view: mounts, usage and block devices.

export function mount(root: HTMLElement, context: FrameContext) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "st";
  box.innerHTML = `
    <div class="st-bar">
      <span class="st-mono">mounts and block devices</span>
      <span class="grow"></span>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="st-mounts"></div>
    <div class="st-table" role="table" aria-label="Block devices"></div>
    <form class="st-add">
      <select data-device aria-label="Device"></select>
      <input data-mountpoint placeholder="mountpoint, e.g. /media/sd" aria-label="Mountpoint" autocomplete="off" />
      <input data-filesystem value="ext4" aria-label="Filesystem" autocomplete="off" />
      <button type="submit">Mount</button>
    </form>
    <p class="st-status" role="status"></p>
  `;
  root.append(box);

  const mounts = box.querySelector(".st-mounts") as any;
  const table = box.querySelector(".st-table") as any;
  const status = box.querySelector(".st-status") as any;
  const form = box.querySelector(".st-add") as any;
  const device = box.querySelector("[data-device]") as any;
  const mountpoint = box.querySelector("[data-mountpoint]") as any;
  const filesystem = box.querySelector("[data-filesystem]") as any;
  let disposed = false;

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "st-status bad" : "st-status";
  };

  const systemMount = (mountpoint: string): boolean =>
    mountpoint === "/" || mountpoint === "/boot" || mountpoint === "/boot/efi";

  async function load() {
    setStatus("Reading mounts and devices…");
    try {
      const [list, devices] = await Promise.all([
        call(context, "/mounts"),
        call(context, "/devices"),
      ]);
      if (disposed) return;
      mounts.replaceChildren();
      for (const mount of list) {
        const card = document.createElement("section");
        card.className = "st-card";
        const head = document.createElement("div");
        head.className = "st-mount-head";
        head.append(
          cell("st-mountpoint", mount.mountpoint),
          cell("st-fs", mount.filesystem),
        );
        if (systemMount(mount.mountpoint)) {
          head.append(cell("st-badge no", "system"));
        } else {
          const unmount = document.createElement("button");
          unmount.type = "button";
          unmount.className = "st-unmount";
          unmount.textContent = "Unmount";
          unmount.title = `Unmount ${mount.device}`;
          unmount.addEventListener("click", () => void detach(mount.device));
          head.append(unmount);
        }
        const deviceLine = cell("st-device", mount.device);
        const ratio = mount["total-bytes"]
          ? mount["used-bytes"] / mount["total-bytes"]
          : 0;
        const track = document.createElement("div");
        track.className = "st-track";
        const fill = document.createElement("div");
        fill.className = `st-fill${ratio > 0.9 ? " full" : ratio > 0.7 ? " warn" : ""}`;
        fill.style.width = `${Math.min(100, Math.round(ratio * 100))}%`;
        track.append(fill);
        const numbers = document.createElement("div");
        numbers.className = "st-numbers";
        numbers.innerHTML = `<span><b>${size(mount["used-bytes"])}</b> used</span><span><b>${size(mount["total-bytes"])}</b> total</span>`;
        card.append(head, deviceLine, track, numbers);
        mounts.append(card);
      }

      table.replaceChildren();
      const head = document.createElement("div");
      head.className = "st-row head";
      for (const label of ["DEVICE", "SIZE", "MODEL", "REMOVABLE", "MOUNTED"]) {
        head.append(cell("", label));
      }
      table.append(head);
      for (const device of devices) {
        const row = document.createElement("div");
        row.className = "st-row";
        const pathCell = cell("st-mono", device.path);
        pathCell.dataset.label = "Device";
        const sizeCell = cell("st-mono", size(device["size-bytes"]));
        sizeCell.dataset.label = "Size";
        const modelCell = cell("", device.model);
        modelCell.dataset.label = "Model";
        const removableCell = cell(
          `st-badge ${device.removable ? "yes" : "no"}`,
          device.removable ? "yes" : "no",
        );
        removableCell.dataset.label = "Removable";
        const mountedCell = cell(
          `st-badge ${device.mounted ? "yes" : "no"}`,
          device.mounted ? "mounted" : "—",
        );
        mountedCell.dataset.label = "Mounted";
        row.append(pathCell, sizeCell, modelCell, removableCell, mountedCell);
        table.append(row);
      }

      const selected = device.value;
      device.replaceChildren();
      for (const candidate of devices.filter((candidate) => !candidate.mounted)) {
        const option = document.createElement("option");
        option.value = candidate.path;
        option.textContent = `${candidate.path} · ${candidate.model}`;
        device.append(option);
      }
      if (devices.some((candidate) => candidate.path === selected && !candidate.mounted)) {
        device.value = selected;
      }
      form.querySelector("button").disabled = device.options.length === 0;
      setStatus(`${list.length} mounts · ${devices.length} devices`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function detach(path) {
    const ok = await confirmAction({
      title: `Unmount ${path}?`,
      message: "Anything still using this mountpoint will lose access to it.",
      confirmLabel: "Unmount",
      danger: true,
    });
    if (!ok) return;
    setStatus(`Unmounting ${path}…`);
    try {
      await call(context, "/unmount", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ device: path }),
      });
      await load();
      setStatus(`Unmounted ${path}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const attached = device.value;
    const target = mountpoint.value.trim();
    if (!attached || !target) {
      setStatus("Pick a device and write a mountpoint.", true);
      return;
    }
    const type = filesystem.value.trim() || "ext4";
    const ok = await confirmAction({
      title: "Mount this device?",
      message: `${attached} will be mounted at ${target} as ${type}.`,
      confirmLabel: "Mount",
    });
    if (!ok) return;
    setStatus(`Mounting ${attached}…`);
    try {
      await call(context, "/mount", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          device: attached,
          mountpoint: target,
          filesystem: type,
        }),
      });
      mountpoint.value = "";
      await load();
      setStatus(`Mounted ${attached} at ${target}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  });

  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  void load();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
