import { call } from "./api";
import { age } from "./state";
import { styles } from "./styles";
import type { ModalContext, Source } from "./types";

const extra = `
  .stm { display: flex; flex-direction: column; gap: 10px; }
  .stm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .stm-status { margin: 0; font-size: 12px; color: var(--muted); }
  .stm-status.bad { color: var(--danger); }
`;

export function mount(root: HTMLElement, context: ModalContext) {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);

  const box = document.createElement("div");
  box.className = "stm";
  box.dataset.testid = "store-sources-modal";
  box.innerHTML = `
    <p class="st-note">
      Sources are configured by the operator in <span class="st-mono">{data}/sources.json</span>.
      The public key there is the catalogue trust root; it cannot come from the index.
    </p>
    <div class="st-sources" data-sources></div>
    <div class="st-actions">
      <button type="button" data-refresh data-testid="store-sources-refresh">Refresh all</button>
      <button type="button" data-close data-testid="store-sources-close">Close</button>
    </div>
    <p class="stm-status" role="status" data-status data-testid="store-sources-status"></p>
  `;
  root.append(box);

  const list = box.querySelector("[data-sources]") as HTMLElement;
  const status = box.querySelector("[data-status]") as HTMLElement;
  let sources: Source[] = [];

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "stm-status bad" : "stm-status";
  };

  function render(): void {
    list.replaceChildren();
    if (sources.length === 0) {
      const empty = document.createElement("div");
      empty.className = "st-empty";
      empty.textContent =
        "No source configured. Add one to {data}/sources.json and reopen this dialog.";
      list.append(empty);
      return;
    }
    for (const source of sources) {
      const row = document.createElement("div");
      row.className = "st-source";
      row.dataset.testid = "store-source";
      row.dataset.sourceId = source.id;
      row.innerHTML = `
        <strong>${escape(source.name)}</strong>
        <span class="st-badge ${source.trusted ? "ok" : "bad"}">${source.trusted ? "trusted" : "unverified"}</span>
        <span class="st-badge">${escape(source.kind)}</span>
        <span class="st-mono" title="${escape(source.url)}">${escape(source.id)}</span>
        <span class="st-note">${escape(age(source["refreshed-at"]))}</span>
      `;
      const refresh = document.createElement("button");
      refresh.type = "button";
      refresh.textContent = "Refresh";
      refresh.dataset.testid = "store-source-refresh";
      refresh.addEventListener("click", () => void refreshOne(source.id));
      row.append(refresh);
      list.append(row);
    }
  }

  async function load(): Promise<void> {
    try {
      sources = (await call(context, "/sources")) ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function refreshOne(id: string): Promise<void> {
    try {
      setStatus(`Refreshing ${id}…`);
      await call(context, "/sources/refresh", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ source: id }),
      });
      setStatus(`${id} refreshed.`);
      await load();
    } catch (error) {
      setStatus(`${id}: ${error}`, true);
    }
  }

  async function refreshAll(): Promise<void> {
    try {
      setStatus("Refreshing every source…");
      let failures = 0;
      for (const source of sources) {
        try {
          await call(context, "/sources/refresh", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ source: source.id }),
          });
        } catch {
          failures += 1;
        }
      }
      await load();
      setStatus(
        failures ? `${failures} source(s) failed; the cache was kept.` : "Catalogues refreshed.",
        failures > 0,
      );
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  box.querySelector("[data-refresh]")?.addEventListener("click", () => void refreshAll());
  box.querySelector("[data-close]")?.addEventListener("click", () => context.close());
  void load();
}

function escape(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
