import { call } from "./api";
import { styles } from "./styles";
import type { ModalContext, ProxyHost } from "./types";

const extra = `
  .pxm { display: flex; flex-direction: column; gap: 12px; }
  .pxm label { display: flex; flex-direction: column; gap: 4px; color: #98a2b6; font-size: 12px; }
  .pxm input[type="text"], .pxm select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 13px;
  }
  .pxm-row { display: flex; gap: 14px; flex-wrap: wrap; }
  .pxm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .pxm button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .pxm button.primary { border-color: #33507a; background: #16233a; color: #dbe6ff; }
  .pxm-status { margin: 0; font-size: 12px; color: #8f98ab; }
  .pxm-status.bad { color: #ffb3ba; }
`;

export function mount(root: HTMLElement, context: ModalContext): () => void {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);

  const box = document.createElement("div");
  box.className = "pxm";
  box.innerHTML = `
    <label>Id<input type="text" name="id" placeholder="site" autofocus required /></label>
    <label>Names<input type="text" name="names" placeholder="example.com, www.example.com" required /></label>
    <label>Certificate<select name="certificate"><option value="">(none)</option></select></label>
    <div class="pxm-row">
      <label class="px-check"><input type="checkbox" name="acme" /> serve ACME HTTP-01</label>
      <label class="px-check"><input type="checkbox" name="redirect" /> redirect HTTP to HTTPS</label>
      <label class="px-check"><input type="checkbox" name="hsts" /> HSTS</label>
    </div>
    <div class="pxm-actions">
      <button type="button" data-cancel>Cancel</button>
      <button type="submit" class="primary" data-submit>Add to draft</button>
    </div>
    <p class="pxm-status" role="status" data-status></p>
  `;
  root.append(box);

  const certificate = box.querySelector(
    'select[name="certificate"]',
  ) as HTMLSelectElement;
  const status = box.querySelector("[data-status]") as HTMLElement;
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  let disposed = false;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message;
    status.className = bad ? "pxm-status bad" : "pxm-status";
  };

  const value = (name: string): string => {
    const input = box.querySelector<HTMLInputElement>(`[name="${name}"]`);
    return input?.value.trim() ?? "";
  };
  const checked = (name: string): boolean =>
    box.querySelector<HTMLInputElement>(`[name="${name}"]`)?.checked ?? false;

  async function loadCertificates(): Promise<void> {
    try {
      const ids = (await call(context, "/certificates")) as string[];
      if (disposed) return;
      for (const id of Array.isArray(ids) ? ids : []) {
        const option = document.createElement("option");
        option.value = id;
        option.textContent = id;
        certificate.append(option);
      }
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function commit(): Promise<void> {
    const id = value("id");
    const names = value("names")
      .split(",")
      .map((name) => name.trim().toLowerCase())
      .filter(Boolean);
    if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
      setStatus("The id is lowercase letters, digits and dashes.", true);
      return;
    }
    if (names.length === 0) {
      setStatus("At least one host name is required.", true);
      return;
    }
    const host: ProxyHost = {
      id,
      names,
      certificate: certificate.value || null,
      acme: checked("acme"),
      "redirect-https": checked("redirect"),
      hsts: checked("hsts"),
      routes: [],
    };
    submit.disabled = true;
    setStatus("Handing the host to the view…");
    try {
      const opened = await context.openView?.({
        tag: "anth-proxy",
        params: { addHost: host },
      });
      if (!opened) {
        submit.disabled = false;
        setStatus("The panel did not hand the host to the view.", true);
        return;
      }
      if (!disposed) context.close();
    } catch (error) {
      submit.disabled = false;
      if (!disposed) setStatus(String(error), true);
    }
  }

  submit.addEventListener("click", () => void commit());
  box.addEventListener("keydown", (event) => {
    if ((event as KeyboardEvent).key === "Enter") {
      event.preventDefault();
      void commit();
    }
  });
  box.querySelector("[data-cancel]")?.addEventListener("click", () => context.close());
  void loadCertificates();
  (box.querySelector('[name="id"]') as HTMLInputElement | null)?.focus();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
