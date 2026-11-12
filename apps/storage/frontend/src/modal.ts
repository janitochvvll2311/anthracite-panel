import { call } from "./api";
import type { ModalContext } from "./types";

export type Device = {
  path: string;
  model: string;
  removable: boolean;
  mounted: boolean;
  "size-bytes": number;
};

const STYLES = `
  .st-modal { display: flex; flex-direction: column; gap: 14px; }
  .st-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .st-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .st-modal form { display: grid; gap: 10px; }
  .st-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .st-modal select, .st-modal input {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12.5px;
  }
  .st-modal select:focus, .st-modal input:focus { outline: 2px solid #7aa2ff; outline-offset: 1px; }
  .st-modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .st-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .st-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .st-modal button:hover:not(:disabled) { border-color: #4a5c82; color: #fff; }
  .st-modal button:disabled { opacity: .5; cursor: default; }
  .st-modal-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .st-modal-status.bad { color: #ffb3ba; }
`;

export function mount(root: HTMLElement, context: ModalContext): () => void {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);

  const box = document.createElement("div");
  box.className = "st-modal";
  box.innerHTML = `
    <div>
      <h3>Mount device</h3>
      <p>Pick an unmounted block device and where it should appear.</p>
    </div>
    <form>
      <label>Device <select data-device aria-label="Device"></select></label>
      <label>Mountpoint
        <input data-mountpoint placeholder="/media/sd" aria-label="Mountpoint" autocomplete="off" />
      </label>
      <label>Filesystem
        <input data-filesystem value="ext4" aria-label="Filesystem" autocomplete="off" />
      </label>
      <div class="st-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Mount</button>
      </div>
    </form>
    <p class="st-modal-status" role="status"></p>
  `;
  root.append(box);

  const form = box.querySelector("form") as HTMLFormElement;
  const device = box.querySelector("[data-device]") as HTMLSelectElement;
  const mountpoint = box.querySelector("[data-mountpoint]") as HTMLInputElement;
  const filesystem = box.querySelector("[data-filesystem]") as HTMLInputElement;
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  submit.disabled = true;
  const cancel = box.querySelector("[data-cancel]") as HTMLButtonElement;
  const status = box.querySelector(".st-modal-status") as HTMLElement;
  let disposed = false;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "st-modal-status bad" : "st-modal-status";
  };

  async function load(): Promise<void> {
    setStatus("Reading block devices…");
    try {
      const devices: Device[] = await call(context, "/devices");
      if (disposed) return;
      device.replaceChildren();
      for (const candidate of devices.filter((candidate) => !candidate.mounted)) {
        const option = document.createElement("option");
        option.value = candidate.path;
        option.textContent = `${candidate.path} · ${candidate.model}`;
        device.append(option);
      }
      submit.disabled = device.options.length === 0;
      setStatus(
        device.options.length === 0
          ? "There are no unmounted devices."
          : `${device.options.length} device(s) ready to mount`,
        device.options.length === 0,
      );
      mountpoint.focus();
    } catch (error) {
      submit.disabled = true;
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
    submit.disabled = true;
    setStatus(`Mounting ${attached}…`);
    try {
      await call(context, "/mount", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          device: attached,
          mountpoint: target,
          filesystem: filesystem.value.trim() || "ext4",
        }),
      });
      if (!disposed) context.close();
    } catch (error) {
      submit.disabled = false;
      if (!disposed) setStatus(String(error), true);
    }
  });

  cancel.addEventListener("click", () => context.close());
  void load();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
