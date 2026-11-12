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

// src/format.ts
function size(bytes) {
  if (bytes >= 1024 ** 4)
    return `${(bytes / 1024 ** 4).toFixed(1)} TiB`;
  if (bytes >= 1024 ** 3)
    return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
  if (bytes >= 1024 ** 2)
    return `${(bytes / 1024 ** 2).toFixed(0)} MiB`;
  return `${bytes} B`;
}
function cell(className, text) {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
}

// src/styles.ts
var styles = `
  .st { display: flex; flex-direction: column; gap: 14px; min-height: 360px; }
  .st-bar { display: flex; align-items: center; gap: 10px; }
  .st-bar .grow { flex: 1; }
  .st-bar button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .st-bar button:hover { border-color: #3a4560; color: #fff; }
  .st-mounts { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 10px; }
  .st-card { border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 11px 12px; display: grid; gap: 8px; }
  .st-mount-head { display: flex; align-items: baseline; gap: 8px; }
  .st-unmount {
    margin-left: auto; padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142;
    background: #151a24; color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer;
  }
  .st-unmount:hover { border-color: #ff7d8a66; color: #ffb3ba; }
  .st-add { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .st-add input, .st-add select, .st-add button {
    padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .st-add button { background: #151a24; cursor: pointer; }
  .st-add button:hover { border-color: #3a4560; color: #fff; }
  .st-add input[data-mountpoint] { flex: 1; min-width: 160px; }
  .st-add input[data-filesystem] { width: 100px; }
  .st-mountpoint { font-weight: 600; color: #e8ebf2; }
  .st-fs { color: #cbb8ff; font: 11px ui-monospace, monospace; }
  .st-device { color: #667087; font: 11px ui-monospace, monospace; }
  .st-track { height: 7px; border-radius: 999px; background: #202634; overflow: hidden; }
  .st-fill { height: 100%; background: linear-gradient(90deg, #7aa2ff, #9d7bff); }
  .st-fill.warn { background: linear-gradient(90deg, #f3c969, #ff9f5a); }
  .st-fill.full { background: linear-gradient(90deg, #ff7d8a, #c2415a); }
  .st-numbers { display: flex; justify-content: space-between; color: #8f98ab; font-size: 11px; }
  .st-numbers b { color: #c8d0df; font-weight: 500; }
  .st-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .st-row {
    display: grid; grid-template-columns: minmax(120px, 1fr) 90px minmax(110px, 1fr) 90px 80px;
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center;
  }
  .st-row:last-child { border-bottom: 0; }
  .st-row.head { background: #11151e; color: #667087; text-transform: uppercase; letter-spacing: .06em; font-size: 10px; }
  .st-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .st-badge { padding: 2px 7px; border-radius: 999px; border: 1px solid #2a3142; font-size: 10px; color: #8f98ab; }
  .st-badge.yes { border-color: #52d1a444; color: #9fe6cb; }
  .st-badge.no { color: #667087; }
  .st-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .st-status.bad { color: #ffb3ba; }

  @media (max-width: 640px) {
    .st-table { overflow: visible; border: 0; background: transparent; }
    .st-row.head { display: none; }
    .st-row {
      display: flex; flex-wrap: wrap; gap: 4px 12px; align-items: center;
      padding: 10px 12px; margin-bottom: 8px;
      border: 1px solid #202632; border-radius: 9px; background: #0d1016;
    }
    .st-row:last-child { margin-bottom: 0; }
    .st-row > span { display: inline-flex; gap: 4px; align-items: baseline; }
    .st-row > span[data-label]::before {
      content: attr(data-label); color: #667087; font-size: 9px;
      text-transform: uppercase; letter-spacing: .06em;
    }
    .st-row > :nth-child(1) { flex: 1 0 100%; color: #e8ebf2; font-weight: 600; }
    .st-row > :nth-child(1)::before { content: none; }
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
  const mounts = box.querySelector(".st-mounts");
  const table = box.querySelector(".st-table");
  const status = box.querySelector(".st-status");
  const form = box.querySelector(".st-add");
  const device = box.querySelector("[data-device]");
  const mountpoint = box.querySelector("[data-mountpoint]");
  const filesystem = box.querySelector("[data-filesystem]");
  const usePanelForms = context.panel?.slots?.includes("panel.fab") === true && context.panel?.slots?.includes("panel.modal") === true;
  if (usePanelForms)
    form.remove();
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "st-status bad" : "st-status";
  };
  const systemMount = (mountpoint) => ["/", "/boot", "/boot/efi", "/proc", "/sys", "/dev", "/run"].some((prefix) => mountpoint === prefix || mountpoint.startsWith(`${prefix}/`));
  async function load() {
    setStatus("Reading mounts and devices…");
    try {
      const [list, devices] = await Promise.all([
        call(context, "/mounts"),
        call(context, "/devices")
      ]);
      if (disposed)
        return;
      mounts.replaceChildren();
      for (const mount of list) {
        const card = document.createElement("section");
        card.className = "st-card";
        const head = document.createElement("div");
        head.className = "st-mount-head";
        head.append(cell("st-mountpoint", mount.mountpoint), cell("st-fs", mount.filesystem));
        if (systemMount(mount.mountpoint)) {
          head.append(cell("st-badge no", "system"));
        } else {
          const unmount = document.createElement("button");
          unmount.type = "button";
          unmount.className = "st-unmount";
          unmount.textContent = "Unmount";
          unmount.title = `Unmount ${mount.device}`;
          unmount.addEventListener("click", () => void detach(mount));
          head.append(unmount);
        }
        const deviceLine = cell("st-device", mount.device);
        const ratio = mount["total-bytes"] ? mount["used-bytes"] / mount["total-bytes"] : 0;
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
        const removableCell = cell(`st-badge ${device.removable ? "yes" : "no"}`, device.removable ? "yes" : "no");
        removableCell.dataset.label = "Removable";
        const mountedCell = cell(`st-badge ${device.mounted ? "yes" : "no"}`, device.mounted ? "mounted" : "—");
        mountedCell.dataset.label = "Mounted";
        row.append(pathCell, sizeCell, modelCell, removableCell, mountedCell);
        table.append(row);
      }
      if (!usePanelForms) {
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
      }
      setStatus(`${list.length} mounts · ${devices.length} devices`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function detach(mount) {
    const ok = await confirmAction({
      title: `Unmount ${mount.mountpoint}?`,
      message: `${mount.device} will no longer be available at this mountpoint.`,
      confirmLabel: "Unmount",
      danger: true
    });
    if (!ok)
      return;
    setStatus(`Unmounting ${mount.mountpoint}…`);
    try {
      await call(context, "/unmount", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ mountpoint: mount.mountpoint })
      });
      await load();
      setStatus(`Unmounted ${mount.mountpoint}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  if (!usePanelForms) {
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
        confirmLabel: "Mount"
      });
      if (!ok)
        return;
      setStatus(`Mounting ${attached}…`);
      try {
        await call(context, "/mount", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            device: attached,
            mountpoint: target,
            filesystem: type
          })
        });
        mountpoint.value = "";
        await load();
        setStatus(`Mounted ${attached} at ${target}`);
      } catch (error) {
        if (!disposed)
          setStatus(String(error), true);
      }
    });
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
