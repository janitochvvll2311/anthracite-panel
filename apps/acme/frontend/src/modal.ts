import { call } from "./api";
import { directoryLabel, isStaging, splitNames } from "./state";
import { styles } from "./styles";
import type {
  Account,
  Capability,
  ModalContext,
  Order,
} from "./types";

// The issue form: it creates the account for the chosen directory when needed,
// then starts the order. The view receives the new order id through `openView`.

const extra = `
  .acm { display: flex; flex-direction: column; gap: 12px; }
  .acm label { display: flex; flex-direction: column; gap: 4px; color: var(--muted); font-size: 12px; }
  .acm input[type="text"], .acm input[type="email"], .acm select {
    width: 100%; padding: 6px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 13px;
  }
  .acm-row { display: flex; gap: 14px; flex-wrap: wrap; }
  .acm-row > label { flex: 1 1 180px; }
  .acm-check { flex-direction: row; align-items: center; gap: 6px; }
  .acm-note { font-size: 11.5px; color: var(--muted); }
  .acm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .acm button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .acm button.primary { border-color: var(--accent); background: var(--panel-3); }
  .acm-status { margin: 0; font-size: 12px; color: var(--muted); }
  .acm-status.bad { color: var(--danger); }
  @media (max-width: 720px) {
    .acm-actions { flex-direction: column-reverse; }
    .acm-actions button { width: 100%; }
  }
`;

function escape(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function status(box: HTMLElement): HTMLElement {
  return box.querySelector("[data-status]") as HTMLElement;
}

function setStatus(box: HTMLElement, message: string, bad = false): void {
  const node = status(box);
  node.textContent = message;
  node.className = bad ? "acm-status bad" : "acm-status";
}

function text(box: HTMLElement, name: string): string {
  return (
    box.querySelector<HTMLInputElement>(`[name="${name}"]`)?.value.trim() ?? ""
  );
}

function checked(box: HTMLElement, name: string): boolean {
  return box.querySelector<HTMLInputElement>(`[name="${name}"]`)?.checked ?? false;
}

export function mount(root: HTMLElement, context: ModalContext) {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);

  const box = document.createElement("div");
  box.className = "acm";
  box.dataset.testid = "acme-issue-modal";
  box.innerHTML = `
    <div class="acm-row">
      <label>CA directory<select name="directory" data-testid="acme-issue-directory"></select></label>
      <label>Contact (when a new account is created)<input type="text" name="contact" data-testid="acme-issue-contact" placeholder="ops@example.com" /></label>
    </div>
    <label>Names<input type="text" name="names" data-testid="acme-issue-names" placeholder="example.com, www.example.com, *.example.com" /></label>
    <div class="acm-row">
      <label>Challenge<select name="challenge" data-testid="acme-issue-challenge">
        <option value="http-01">HTTP-01</option>
        <option value="dns-01">DNS-01</option>
      </select></label>
      <label>Solver<select name="solver" data-testid="acme-issue-solver"></select></label>
      <label>Key type<select name="key-type" data-testid="acme-issue-key-type">
        <option value="ecdsa-p256">ECDSA P-256</option>
        <option value="rsa-2048">RSA 2048</option>
      </select></label>
    </div>
    <label class="acm-check"><input type="checkbox" name="auto-renew" data-testid="acme-issue-auto-renew" checked /> renew automatically from the certificate store</label>
    <label class="acm-check"><input type="checkbox" name="force" data-testid="acme-issue-force" /> allow a duplicate order (reissue without revoking)</label>
    <p class="acm-note" data-note></p>
    <div class="acm-actions">
      <button type="button" data-cancel data-testid="acme-modal-cancel">Cancel</button>
      <button type="button" class="primary" data-submit data-testid="acme-modal-submit">Issue certificate</button>
    </div>
    <p class="acm-status" role="status" data-status data-testid="acme-modal-status"></p>
  `;
  root.append(box);

  let accounts: Account[] = [];
  let capabilities: Capability[] = [];
  let directories: string[] = [];
  let stagingDirectories: string[] = [];
  let solvers: string[] = [];
  let chosen = false;

  const directorySelect = box.querySelector(
    '[name="directory"]',
  ) as HTMLSelectElement;
  const challengeSelect = box.querySelector(
    '[name="challenge"]',
  ) as HTMLSelectElement;
  const solverSelect = box.querySelector('[name="solver"]') as HTMLSelectElement;
  const note = box.querySelector("[data-note]") as HTMLElement;

  function staging(url: string): boolean {
    return isStaging(capabilities, url);
  }

  function updateNote(): void {
    const url = directorySelect.value;
    const existing = accounts.find((account) => account.directory === url);
    const parts = [staging(url) ? "staging CA: never promoted" : "production CA"];
    parts.push(
      existing
        ? `using account ${existing.id}`
        : "a new account is created for this directory",
    );
    if (challengeSelect.value === "dns-01") {
      parts.push("DNS-01 needs a solver; manual waits for your confirmation");
    }
    note.textContent = parts.join(" · ");
  }

  function updateSolvers(): void {
    const dns = challengeSelect.value === "dns-01";
    solverSelect.disabled = !dns;
    solverSelect.innerHTML = dns
      ? solvers.map((id) => `<option value="${escape(id)}">${escape(id)}</option>`).join("")
      : `<option value="">(not used by HTTP-01)</option>`;
    updateNote();
  }

  function fillOptions(): void {
    directorySelect.innerHTML = directories
      .map(
        (url) =>
          `<option value="${escape(url)}">${escape(directoryLabel(url))}${
            staging(url) ? " (staging)" : ""
          }</option>`,
      )
      .join("");
    updateSolvers();
  }

  async function submit(): Promise<void> {
    const button = box.querySelector("[data-submit]") as HTMLButtonElement;
    button.disabled = true;
    try {
      const url = directorySelect.value;
      const names = splitNames(text(box, "names"));
      if (names.length === 0) {
        setStatus(box, "Add at least one DNS name.", true);
        return;
      }
      if (!url) {
        setStatus(box, "Choose a CA directory.", true);
        return;
      }
      const challenge = challengeSelect.value;
      if (
        challenge === "http-01" &&
        names.some((name) => name.startsWith("*."))
      ) {
        setStatus(box, "A wildcard needs DNS-01.", true);
        return;
      }
      const keyType = (
        box.querySelector('[name="key-type"]') as HTMLSelectElement
      ).value;
      let account = accounts.find((entry) => entry.directory === url);
      if (!account) {
        setStatus(box, "Creating the account…");
        account = await call(context, "/accounts", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            directory: url,
            contact: [text(box, "contact")].filter(Boolean),
            "key-type": keyType,
          }),
        });
      }
      setStatus(box, "Starting the order…");
      const order: Order = await call(context, "/orders", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          account: account.id,
          names,
          challenge,
          solver: challenge === "dns-01" ? solverSelect.value : null,
          "key-type": keyType,
          "auto-renew": checked(box, "auto-renew"),
          staging: staging(url),
          force: checked(box, "force"),
        }),
      });
      const opened = await context.openView?.({
        tag: "anth-acme",
        params: { order: order.id },
      });
      if (!opened) {
        setStatus(box, "The panel did not open the order in the view.", true);
        return;
      }
      context.close();
    } catch (error) {
      setStatus(box, String(error), true);
    } finally {
      button.disabled = false;
    }
  }

  box.querySelector("[data-cancel]")?.addEventListener("click", () => {
    context.close();
  });
  box.querySelector("[data-submit]")?.addEventListener("click", () => {
    void submit();
  });
  directorySelect.addEventListener("change", () => {
    chosen = true;
    updateNote();
  });
  challengeSelect.addEventListener("change", updateSolvers);

  void (async () => {
    try {
      const [nextAccounts, nextCapabilities] = await Promise.all([
        call(context, "/accounts"),
        call(context, "/capabilities"),
      ]);
      accounts = nextAccounts ?? [];
      capabilities = nextCapabilities ?? [];
      directories = capabilities[0]?.directories ?? [];
      stagingDirectories = capabilities[0]?.["staging-directories"] ?? [];
      solvers = capabilities.find((entry) => entry.challenge === "dns-01")?.solvers ?? [];
      if (directories.length === 0) {
        setStatus(box, "The service has no CA directory configured.", true);
        return;
      }
      // Prefer a production directory by default; staging stays an explicit
      // choice in the select.
      const firstProduction = directories.find((url) => !staging(url));
      fillOptions();
      // The async load must not overwrite a directory the operator already
      // chose while the capabilities were arriving.
      if (!chosen && firstProduction) directorySelect.value = firstProduction;
      updateSolvers();
    } catch (error) {
      setStatus(box, String(error), true);
    }
  })();
}
