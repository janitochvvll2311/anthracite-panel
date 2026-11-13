// The panel's Manager: installed apps, grants and the composition.
//
// This is panel chrome, not an app module. App modules run in a sandboxed frame
// and may only call their own `/api/apps/{id}/` prefix; managing the engine is
// the host's job and uses the same session as the rest of the shell.

import { authorityDiff, RISKS, type Risk } from "./authority";

type AppView = {
  id: string;
  version: string;
  enabled: boolean;
  alias?: string;
  signer?: string;
  tier: "third-party" | "system";
  installed_at: string;
  grant_state: "none" | "current" | "stale" | "invalid";
  grant_problem?: string;
  grant?: {
    risk: string;
    accepted: string[];
    approved_by?: string;
    approved_at?: string;
  };
};

type AppList = {
  apps: AppView[];
  problems: { app: string; message: string }[];
};

type Composition = {
  version: number;
  profile: { id: string; apps: string[] }[];
};

type CompositionView = {
  composition: Composition;
};

type ApprovalPreview = {
  risk: string;
  required: Risk[];
};

type ApprovalState =
  | { status: "loading" }
  | { status: "ready"; risk: string; required: Risk[]; grant?: AppView["grant"] }
  | { status: "error"; message: string };

type AuditRow = {
  at: string;
  actor: string;
  action: string;
  result: string;
  app?: string;
  detail?: unknown;
};

type AuditPage = {
  entries: AuditRow[];
  total: number;
  next: number | null;
  problems: string[];
};

export type ManagerOptions = {
  request: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
  onChanged: () => Promise<void>;
};

class ManagerError extends Error {
  detail?: unknown;
  constructor(message: string, detail?: unknown) {
    super(message);
    this.detail = detail;
  }
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function button(className: string, text: string, title?: string): HTMLButtonElement {
  const node = element("button", className, text);
  node.type = "button";
  if (title) node.title = title;
  return node;
}

function badge(text: string, kind = ""): HTMLSpanElement {
  return element("span", `mgr-badge${kind ? ` ${kind}` : ""}`, text);
}

function reason(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function api(
  request: ManagerOptions["request"],
  path: string,
  init?: RequestInit,
): Promise<{ response: Response; body: any }> {
  const response = await request(path, init);
  const text = await response.text();
  let body: any = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = text;
  }
  if (!response.ok) {
    const error = body && typeof body === "object" ? body.error : null;
    const message = error
      ? `${error.code}: ${error.message}`
      : `HTTP ${response.status}`;
    throw new ManagerError(message, error?.detail);
  }
  return { response, body };
}

export function mountManager(
  root: HTMLElement,
  options: ManagerOptions,
): () => void {
  let apps: AppView[] = [];
  let problems: AppList["problems"] = [];
  let composition: Composition = {
    version: 1,
    profile: [{ id: "default", apps: [] }],
  };
  let etag = "";
  let pendingApprove: string | null = null;
  let approval: ApprovalState | null = null;
  let audit: AuditRow[] = [];
  let auditNext: number | null = null;
  let auditProblems: string[] = [];
  let auditLoading = false;
  let disposed = false;

  root.innerHTML = `
    <section class="mgr-section">
      <header>
        <strong>Installed apps</strong>
        <div class="mgr-form">
          <label class="mgr-check"><input type="checkbox" data-ack /> Acknowledge dependents</label>
          <button class="mgr-btn" type="button" data-refresh>Refresh</button>
        </div>
      </header>
      <div class="mgr-section-body">
        <div data-apps></div>
        <div class="mgr-approve" data-approve hidden></div>
        <div class="mgr-form">
          <input type="file" accept=".anth,application/zip" data-install-file />
          <button class="mgr-btn primary" type="button" data-install>Install bundle</button>
          <span class="mgr-hint">or upgrade an installed app:</span>
          <select class="mgr-select" data-upgrade-id></select>
          <input type="file" accept=".anth,application/zip" data-upgrade-file />
          <button class="mgr-btn" type="button" data-upgrade>Upgrade</button>
        </div>
      </div>
    </section>

    <section class="mgr-section">
      <header>
        <strong>Composition</strong>
        <span class="mgr-hint">saved now; running apps change on restart</span>
      </header>
      <div class="mgr-section-body">
        <div class="mgr-compose" data-compose></div>
        <div class="mgr-form">
          <button class="mgr-btn primary" type="button" data-save-compose>Save composition</button>
        </div>
      </div>
    </section>

    <section class="mgr-section">
      <header>
        <strong>Audit</strong>
        <div class="mgr-form">
          <button class="mgr-btn" type="button" data-audit-refresh>Refresh</button>
        </div>
      </header>
      <div class="mgr-section-body">
        <div data-audit></div>
        <div class="mgr-form" data-audit-more hidden>
          <button class="mgr-btn" type="button" data-audit-older>Load older</button>
        </div>
      </div>
    </section>

    <p class="mgr-status" data-status role="status"></p>
  `;

  const appsRoot = root.querySelector<HTMLElement>("[data-apps]")!;
  const composeRoot = root.querySelector<HTMLElement>("[data-compose]")!;
  const auditRoot = root.querySelector<HTMLElement>("[data-audit]")!;
  const auditMore = root.querySelector<HTMLElement>("[data-audit-more]")!;
  const status = root.querySelector<HTMLElement>("[data-status]")!;
  const approveRoot = root.querySelector<HTMLElement>("[data-approve]")!;
  const ack = root.querySelector<HTMLInputElement>("[data-ack]")!;
  const installFile = root.querySelector<HTMLInputElement>("[data-install-file]")!;
  const upgradeFile = root.querySelector<HTMLInputElement>("[data-upgrade-file]")!;
  const upgradeId = root.querySelector<HTMLSelectElement>("[data-upgrade-id]")!;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message;
    status.className = bad ? "mgr-status bad" : "mgr-status";
  };

  const ackHeaders = (): HeadersInit =>
    ack.checked ? { "x-anth-acknowledge": "dependents" } : {};

  async function refresh(silent = false): Promise<void> {
    if (!silent) setStatus("Reading the engine…");
    try {
      const [list, current] = await Promise.all([
        api(options.request, "/api/engine/apps"),
        api(options.request, "/api/engine/composition"),
      ]);
      if (disposed) return;
      apps = (list.body as AppList).apps ?? [];
      problems = (list.body as AppList).problems ?? [];
      const view = current.body as CompositionView;
      composition = view.composition;
      etag = current.response.headers.get("etag") ?? "";
      if (!silent) setStatus(`${apps.length} installed app(s)`);
      renderApps();
      renderComposition();
      void loadAudit();
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  function renderApps(): void {
    appsRoot.replaceChildren();
    if (apps.length === 0) {
      appsRoot.append(element("p", "mgr-hint", "No apps are installed."));
    } else {
      const table = element("table", "mgr-table");
      const head = element("tr", "mgr-head");
      for (const label of ["APP", "VERSION", "STATE", "GRANT", "ACTIONS"]) {
        head.append(element("th", "", label));
      }
      table.append(head);
      for (const app of apps) {
        const row = element("tr");
        const id = element("td", "mgr-app");
        id.dataset.label = "App";
        id.append(element("div", "mgr-mono", app.id));
        if (app.alias) id.append(element("div", "mgr-mono", `alias ${app.alias}`));
        row.append(id);

        const version = element("td", "mgr-version mgr-mono", app.version);
        version.dataset.label = "Version";
        row.append(version);

        const state = element("td", "mgr-state");
        state.dataset.label = "State";
        state.append(
          badge(app.enabled ? "enabled" : "disabled", app.enabled ? "ok" : "warn"),
          document.createTextNode(" "),
          badge(app.tier === "system" ? "system" : "third-party"),
        );
        row.append(state);

        const grant = element("td", "mgr-grant");
        grant.dataset.label = "Grant";
        const kind =
          app.grant_state === "current"
            ? "ok"
            : app.grant_state === "none"
              ? ""
              : "bad";
        grant.append(badge(app.grant_state, kind));
        if (app.grant) {
          grant.append(
            document.createTextNode(" "),
            badge(`risk ${app.grant.risk}`, "warn"),
          );
        }
        row.append(grant);

        const actions = element("td", "mgr-actions-cell");
        actions.dataset.label = "Actions";
        const tools = element("div", "mgr-actions");
        const toggleButton = button(
          "mgr-btn",
          app.enabled ? "Disable" : "Enable",
          app.enabled ? "Disable this app" : "Enable this app",
        );
        toggleButton.addEventListener("click", () => void patch(app, !app.enabled));
        const approveButton = button("mgr-btn primary", "Approve", "Approve a grant");
        approveButton.addEventListener("click", () => {
          if (pendingApprove === app.id) {
            pendingApprove = null;
            approval = null;
            renderApps();
            return;
          }
          pendingApprove = app.id;
          approval = { status: "loading" };
          renderApps();
          void loadApproval(app.id);
        });
        const removeButton = button("mgr-btn danger", "Remove", "Remove this app");
        removeButton.addEventListener("click", () => void remove(app));
        tools.append(toggleButton, approveButton);
        if (app.grant_state !== "none") {
          const revokeButton = button("mgr-btn danger", "Revoke", "Revoke the grant");
          revokeButton.addEventListener("click", () => void revoke(app));
          tools.append(revokeButton);
        }
        tools.append(removeButton);
        actions.append(tools);
        row.append(actions);
        table.append(row);
      }
      appsRoot.append(table);
    }

    for (const problem of problems) {
      appsRoot.append(
        element(
          "p",
          "mgr-hint",
          `unreadable ${problem.app}: ${problem.message}`,
        ),
      );
    }

    renderApprove();
    renderUpgradeOptions();
  }

  async function loadApproval(id: string): Promise<void> {
    try {
      const [detail, preview] = await Promise.all([
        api(options.request, `/api/engine/apps/${encodeURIComponent(id)}`),
        api(options.request, `/api/engine/apps/${encodeURIComponent(id)}/approval`),
      ]);
      if (disposed || pendingApprove !== id) return;
      const risk = preview.body as ApprovalPreview;
      approval = {
        status: "ready",
        risk: risk.risk,
        required: risk.required ?? [],
        grant: (detail.body as AppView).grant,
      };
      renderApps();
    } catch (error) {
      if (!disposed && pendingApprove === id) {
        approval = { status: "error", message: reason(error) };
        renderApps();
      }
    }
  }

  function renderApprove(): void {
    approveRoot.replaceChildren();
    approveRoot.hidden = pendingApprove === null;
    if (!pendingApprove) return;
    const app = apps.find((candidate) => candidate.id === pendingApprove);
    if (!app) {
      pendingApprove = null;
      approveRoot.hidden = true;
      return;
    }
    approveRoot.append(element("strong", "mgr-mono mgr-approve-title", `Approve ${app.id}`));
    const close = (): void => {
      pendingApprove = null;
      approval = null;
      renderApps();
    };
    const cancel = button("mgr-btn", "Cancel");
    cancel.addEventListener("click", close);
    const showActions = (...buttons: HTMLButtonElement[]): void => {
      const actions = element("div", "mgr-approve-actions");
      actions.append(...buttons, cancel);
      approveRoot.append(actions);
      approveRoot.scrollIntoView({ block: "nearest", behavior: "smooth" });
    };
    if (!approval || approval.status === "loading") {
      approveRoot.append(element("span", "mgr-hint", "Reading the approval…"));
      showActions();
      return;
    }
    if (approval.status === "error") {
      approveRoot.append(
        element("span", "mgr-hint", `Could not read the approval: ${approval.message}`),
      );
      const retry = button("mgr-btn primary", "Retry");
      retry.addEventListener("click", () => {
        approval = { status: "loading" };
        renderApps();
        void loadApproval(app.id);
      });
      showActions(retry);
      return;
    }
    const grant = approval.grant;
    const diff = authorityDiff(grant, approval.risk, approval.required);
    const summary = element("div", "mgr-diff");
    const diffRow = (label: string, value: string, kind = "") => {
      const row = element("div", "mgr-diff-row");
      row.append(element("span", "mgr-diff-label", label), badge(value, kind));
      return row;
    };
    summary.append(
      diffRow("Current grant", diff.current, diff.current === "none" ? "" : "ok"),
      diffRow("Effective risk", diff.effective, diff.effective === "none" ? "" : "warn"),
      diffRow(
        "New authority",
        diff.newLevels.length > 0 ? diff.newLevels.join(", ") : "none",
        diff.newLevels.length > 0 ? "warn" : "ok",
      ),
    );
    if (grant?.approved_by) {
      summary.append(
        element(
          "p",
          "mgr-hint",
          `Approved by ${grant.approved_by}${
            grant.approved_at ? ` at ${grant.approved_at}` : ""
          }`,
        ),
      );
    }
    approveRoot.append(summary);
    const risks = element("div", "mgr-risks");
    const chosen: HTMLInputElement[] = [];
    for (const risk of RISKS) {
      const label = element("label", "mgr-check");
      const input = document.createElement("input");
      input.type = "checkbox";
      input.value = risk;
      if (grant?.accepted.includes(risk)) input.checked = true;
      if (diff.required.includes(risk)) input.title = "required by the effective risk";
      chosen.push(input);
      label.append(input, document.createTextNode(` ${risk}`));
      risks.append(label);
    }
    approveRoot.append(risks);
    const confirm = button("mgr-btn primary", "Approve");
    confirm.addEventListener("click", () => {
      const selected = chosen
        .filter((input) => input.checked)
        .map((input) => input.value as Risk);
      const missing = diff.required.filter((level) => !selected.includes(level));
      if (missing.length > 0) {
        setStatus(`Approve requires: ${missing.join(", ")}`, true);
        return;
      }
      void approve(app, selected);
    });
    showActions(confirm);
    approveRoot.append(
      element("span", "mgr-hint", "read is implicit; check every level the risk needs"),
    );
  }

  function renderUpgradeOptions(): void {
    const selected = upgradeId.value;
    upgradeId.replaceChildren();
    for (const app of apps) {
      const option = document.createElement("option");
      option.value = app.id;
      option.textContent = app.id;
      upgradeId.append(option);
    }
    if (apps.some((app) => app.id === selected)) upgradeId.value = selected;
  }

  function renderComposition(): void {
    composeRoot.replaceChildren();
    const current = composition.profile[0]?.apps ?? [];
    for (const app of apps) {
      const label = element("label");
      const input = document.createElement("input");
      input.type = "checkbox";
      input.dataset.app = app.id;
      input.checked = current.includes(app.id);
      label.append(input, document.createTextNode(` ${app.id}`));
      composeRoot.append(label);
    }
    if (apps.length === 0) {
      composeRoot.append(element("p", "mgr-hint", "Install an app first."));
    }
  }

  async function loadAudit(older = false): Promise<void> {
    if (auditLoading || disposed) return;
    auditLoading = true;
    try {
      const path =
        older && auditNext !== null
          ? `/api/engine/audit?before=${auditNext}`
          : "/api/engine/audit";
      const { body } = await api(options.request, path);
      if (disposed) return;
      const page = body as AuditPage;
      const entries = page.entries ?? [];
      audit = older ? [...audit, ...entries] : entries;
      auditNext = page.next ?? null;
      auditProblems = page.problems ?? [];
      renderAudit();
    } catch (error) {
      if (!disposed) {
        auditRoot.replaceChildren(
          element("p", "mgr-hint", `Could not read the audit log: ${reason(error)}`),
        );
        auditMore.hidden = true;
      }
    } finally {
      auditLoading = false;
    }
  }

  function renderAudit(): void {
    auditRoot.replaceChildren();
    if (audit.length === 0) {
      auditRoot.append(element("p", "mgr-hint", "No audit entries."));
    } else {
      const table = element("table", "mgr-table");
      const head = element("tr", "mgr-head");
      for (const label of ["AT", "ACTOR", "ACTION", "RESULT", "APP", "DETAIL"]) {
        head.append(element("th", "", label));
      }
      table.append(head);
      for (const entry of audit) {
        const row = element("tr");
        const at = element("td", "mgr-mono", entry.at);
        at.dataset.label = "At";
        const actor = element("td", "mgr-mono", entry.actor);
        actor.dataset.label = "Actor";
        const action = element("td", "mgr-mono", entry.action);
        action.dataset.label = "Action";
        const result = element("td", "");
        result.dataset.label = "Result";
        result.append(
          badge(
            entry.result,
            entry.result === "ok" ? "ok" : entry.result === "denied" ? "bad" : "warn",
          ),
        );
        const app = element("td", "mgr-mono", entry.app ?? "—");
        app.dataset.label = "App";
        const detailText =
          entry.detail === undefined ? "" : JSON.stringify(entry.detail);
        const detail = element("td", "mgr-mono mgr-audit-detail", detailText || "—");
        detail.dataset.label = "Detail";
        detail.title = detailText;
        row.append(at, actor, action, result, app, detail);
        table.append(row);
      }
      auditRoot.append(table);
    }
    for (const problem of auditProblems) {
      auditRoot.append(element("p", "mgr-hint", `unreadable audit line: ${problem}`));
    }
    auditMore.hidden = auditNext === null;
  }

  async function patch(app: AppView, enabled: boolean): Promise<void> {
    setStatus(`${enabled ? "Enabling" : "Disabling"} ${app.id}…`);
    try {
      await api(options.request, `/api/engine/apps/${encodeURIComponent(app.id)}`, {
        method: "PATCH",
        headers: { "content-type": "application/json", ...ackHeaders() },
        body: JSON.stringify({ enabled }),
      });
      setStatus(`${app.id} ${enabled ? "enabled" : "disabled"}`);
      await options.onChanged();
      await refresh(true);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function approve(app: AppView, accept: Risk[]): Promise<void> {
    setStatus(`Approving ${app.id}…`);
    try {
      await api(options.request, `/api/engine/apps/${encodeURIComponent(app.id)}/grant`, {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ accept }),
      });
      pendingApprove = null;
      approval = null;
      setStatus(`${app.id} approved`);
      await options.onChanged();
      await refresh(true);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function revoke(app: AppView): Promise<void> {
    setStatus(`Revoking ${app.id}…`);
    try {
      await api(options.request, `/api/engine/apps/${encodeURIComponent(app.id)}/grant`, {
        method: "DELETE",
        headers: ackHeaders(),
      });
      setStatus(`${app.id} revoked`);
      await options.onChanged();
      await refresh(true);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function remove(app: AppView): Promise<void> {
    setStatus(`Removing ${app.id}…`);
    try {
      await api(options.request, `/api/engine/apps/${encodeURIComponent(app.id)}`, {
        method: "DELETE",
        headers: ackHeaders(),
      });
      setStatus(`${app.id} removed`);
      await options.onChanged();
      await refresh(true);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function install(file: File): Promise<void> {
    setStatus(`Installing ${file.name}…`);
    try {
      await api(options.request, "/api/engine/apps", {
        method: "POST",
        headers: { "content-type": "application/zip" },
        body: file,
      });
      setStatus(`${file.name} installed`);
      await options.onChanged();
      await refresh(true);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function upgrade(id: string, file: File): Promise<void> {
    setStatus(`Upgrading ${id}…`);
    try {
      await api(options.request, `/api/engine/apps/${encodeURIComponent(id)}`, {
        method: "PUT",
        headers: { "content-type": "application/zip", ...ackHeaders() },
        body: file,
      });
      setStatus(`${id} upgraded`);
      await options.onChanged();
      await refresh(true);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function saveComposition(): Promise<void> {
    const selected = [...composeRoot.querySelectorAll<HTMLInputElement>("input[data-app]")]
      .filter((input) => input.checked)
      .map((input) => input.dataset.app as string);
    const document = {
      version: composition.version,
      profile: [{ id: composition.profile[0]?.id ?? "default", apps: selected }],
    };
    setStatus("Saving the composition…");
    try {
      await api(options.request, "/api/engine/composition", {
        method: "PUT",
        headers: { "content-type": "application/json", "if-match": etag },
        body: JSON.stringify(document),
      });
      setStatus("Composition saved; restart the panel to apply it");
      await options.onChanged();
      await refresh(true);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  root.querySelector("[data-refresh]")!.addEventListener("click", () => void refresh());
  root.querySelector("[data-install]")!.addEventListener("click", () => {
    const file = installFile.files?.[0];
    if (!file) {
      setStatus("Choose a bundle first", true);
      return;
    }
    void install(file).finally(() => {
      installFile.value = "";
    });
  });
  root.querySelector("[data-upgrade]")!.addEventListener("click", () => {
    const file = upgradeFile.files?.[0];
    if (!file) {
      setStatus("Choose a bundle first", true);
      return;
    }
    void upgrade(upgradeId.value, file).finally(() => {
      upgradeFile.value = "";
    });
  });
  root.querySelector("[data-save-compose]")!.addEventListener("click", () => {
    void saveComposition();
  });
  root.querySelector("[data-audit-refresh]")!.addEventListener("click", () => {
    void loadAudit();
  });
  root.querySelector("[data-audit-older]")!.addEventListener("click", () => {
    void loadAudit(true);
  });

  void refresh();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
