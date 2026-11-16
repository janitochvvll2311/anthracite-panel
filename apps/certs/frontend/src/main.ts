import { call } from "./api";
import { styles } from "./styles";
import type { Certificate, FrameContext } from "./types";

// The certificates view: inventory, expiry, chain state and publication.

function date(seconds: number): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleString();
}

function days(seconds: number): number {
  return Math.ceil((seconds * 1000 - Date.now()) / 86_400_000);
}

export function mount(root: HTMLElement, context: FrameContext) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "ct";
  box.innerHTML = `
    <div class="ct-bar">
      <span class="ct-mono">managed certificates and scan roots</span>
      <span class="grow"></span>
      <button type="button" data-import>Import</button>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="ct-summary" data-summary></div>
    <div class="ct-table" role="table" aria-label="Certificates"></div>
    <section class="ct-detail" data-detail hidden></section>
    <p class="ct-status" role="status"></p>
  `;
  root.append(box);

  const table = box.querySelector(".ct-table") as HTMLElement;
  const summary = box.querySelector("[data-summary]") as HTMLElement;
  const detail = box.querySelector("[data-detail]") as HTMLElement;
  const status = box.querySelector(".ct-status") as HTMLElement;
  const importButton = box.querySelector("[data-import]") as HTMLButtonElement;
  const refresh = box.querySelector("[data-refresh]") as HTMLButtonElement;
  const hasFab = context.panel?.slots?.includes("panel.fab") === true;
  const hasModal = context.panel?.slots?.includes("panel.modal") === true;
  if (!hasFab || !hasModal) {
    importButton.hidden = true;
  }
  let certificates: Certificate[] = [];
  let selected: string | null = null;
  let disposed = false;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "ct-status bad" : "ct-status";
  };

  const badge = (label: string, kind: string): HTMLElement => {
    const node = document.createElement("span");
    node.className = `ct-badge ${kind}`;
    node.textContent = label;
    return node;
  };

  const statusKind = (status: string): string => {
    if (status === "valid") return "ok";
    if (status === "expiring") return "warn";
    if (status === "expired") return "bad";
    return "off";
  };

  function renderSummary(): void {
    const expiring = certificates.filter((cert) => cert.status === "expiring").length;
    const expired = certificates.filter((cert) => cert.status === "expired").length;
    summary.replaceChildren(
      badge(`${certificates.length} certificates`, "off"),
      badge(`${expiring} expiring`, expiring ? "warn" : "off"),
      badge(`${expired} expired`, expired ? "bad" : "off"),
    );
  }

  function renderDetail(cert: Certificate): void {
    selected = cert.id;
    detail.hidden = false;
    detail.replaceChildren();
    const title = document.createElement("h3");
    title.textContent = cert.names[0] ?? cert.id;
    const grid = document.createElement("dl");
    const row = (label: string, value: string): void => {
      const dt = document.createElement("dt");
      dt.textContent = label;
      const dd = document.createElement("dd");
      dd.textContent = value;
      grid.append(dt, dd);
    };
    row("Issuer", cert.issuer);
    row("Names", cert.names.join(", ") || "—");
    row("Validity", `${date(cert["not-before"])} → ${date(cert["not-after"])}`);
    row("Key", cert["key-type"]);
    row("Source", cert.source);
    row("Fingerprint", cert["fingerprint-sha256"]);
    row("Status", cert.status);
    const actions = document.createElement("div");
    actions.className = "ct-actions";
    const validate = document.createElement("button");
    validate.type = "button";
    validate.textContent = "Validate";
    const publish = document.createElement("button");
    publish.type = "button";
    publish.textContent = "Publish";
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Remove";
    remove.className = "danger";
    const close = document.createElement("button");
    close.type = "button";
    close.textContent = "Close";
    actions.append(validate, publish, remove, close);
    if (cert.source === "acme") {
      const renew = document.createElement("label");
      renew.className = "ct-check";
      const input = document.createElement("input");
      input.type = "checkbox";
      input.checked = cert["auto-renew"];
      input.addEventListener("change", async () => {
        try {
          await call(context, "/certificate/auto-renew", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ id: cert.id, enabled: input.checked }),
          });
          setStatus(input.checked ? "Automatic renewal enabled." : "Automatic renewal disabled.");
          await load();
        } catch (error) {
          input.checked = !input.checked;
          setStatus(String(error), true);
        }
      });
      renew.append(input, document.createTextNode(" auto-renew"));
      actions.append(renew);
    }
    detail.append(title, grid, actions);

    validate.addEventListener("click", async () => {
      setStatus("Validating…");
      try {
        const result: any = await call(context, "/certificate/validate", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id: cert.id }),
        });
        setStatus(
          result.ok
            ? "Valid: the chain and key are consistent."
            : `Invalid: ${result.reason ?? "unknown reason"}`,
          !result.ok,
        );
      } catch (error) {
        setStatus(String(error), true);
      }
    });
    publish.addEventListener("click", async () => {
      setStatus("Publishing…");
      try {
        await call(context, "/certificate/publish", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id: cert.id }),
        });
        setStatus("Published to {shared}/tls.");
      } catch (error) {
        setStatus(String(error), true);
      }
    });
    let confirming = false;
    remove.addEventListener("click", async () => {
      if (!confirming) {
        confirming = true;
        remove.textContent = "Confirm remove";
        setStatus("The published copy stays until the proxy reloads.");
        return;
      }
      try {
        await call(context, `/certificate?id=${encodeURIComponent(cert.id)}`, {
          method: "DELETE",
        });
        setStatus("Removed from the managed store; the published copy is kept.");
        detail.hidden = true;
        await load();
      } catch (error) {
        setStatus(String(error), true);
      }
    });
    close.addEventListener("click", () => {
      detail.hidden = true;
      selected = null;
    });
  }

  function render(): void {
    renderSummary();
    table.replaceChildren();
    if (certificates.length === 0) {
      const empty = document.createElement("p");
      empty.className = "ct-empty";
      empty.textContent = "No certificates found in the scan roots or the managed store.";
      table.append(empty);
      return;
    }
    const head = document.createElement("div");
    head.className = "ct-row head";
    for (const label of ["Name", "Status", "Expires", "Source", "Key", "Renew"]) {
      const cell = document.createElement("span");
      cell.textContent = label;
      head.append(cell);
    }
    table.append(head);
    for (const cert of certificates) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "ct-row";
      if (cert.id === selected) row.classList.add("selected");
      const name = document.createElement("span");
      name.textContent = cert.names[0] ?? cert.id;
      const state = document.createElement("span");
      state.append(badge(cert.status, statusKind(cert.status)));
      const expiry = document.createElement("span");
      const left = days(cert["not-after"]);
      expiry.textContent = left < 0 ? "expired" : `${left} d`;
      const source = document.createElement("span");
      source.textContent = cert.source;
      const key = document.createElement("span");
      key.textContent = cert["key-type"];
      const renew = document.createElement("span");
      renew.textContent = cert["auto-renew"] ? "on" : "—";
      row.append(name, state, expiry, source, key, renew);
      row.addEventListener("click", () => renderDetail(cert));
      table.append(row);
    }
  }

  async function load(): Promise<void> {
    setStatus("Reading inventory…");
    try {
      const list: any = await call(context, "/certs");
      if (disposed) return;
      certificates = Array.isArray(list) ? list : [];
      render();
      if (selected) {
        const current = certificates.find((cert) => cert.id === selected);
        if (current) renderDetail(current);
        else detail.hidden = true;
      }
      const errors = certificates.filter((cert) => cert.status === "invalid").length;
      setStatus(
        `Read ${certificates.length} certificate(s)${errors ? `; ${errors} invalid` : ""}.`,
      );
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  importButton.addEventListener("click", () => {
    void context.openModal?.({ tag: "certs-import-modal" });
  });
  refresh.addEventListener("click", () => void load());
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") void load();
  });
  const offVisibility = context.onVisibility?.((visible) => {
    if (visible) void load();
  });
  void load();

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    root.replaceChildren();
  };
}
