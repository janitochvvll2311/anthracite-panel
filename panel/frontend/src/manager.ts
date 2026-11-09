// The panel's Manager: installed apps, grants and the composition.
//
// This is panel chrome, not an app module. App modules run in a sandboxed frame
// and may only call their own `/api/apps/{id}/` prefix; managing the engine is
// the host's job and uses the same session as the rest of the shell.

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
  grant?: { risk: string; accepted: string[] };
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
  surface: { apps: unknown[]; degraded: unknown[] };
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

const RISKS = ["write", "admin", "host", "package"] as const;
type Risk = (typeof RISKS)[number];

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

    <p class="mgr-status" data-status role="status"></p>
  `;

  const appsRoot = root.querySelector<HTMLElement>("[data-apps]")!;
  const composeRoot = root.querySelector<HTMLElement>("[data-compose]")!;
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
      composition = (current.body as CompositionView).composition;
      etag = current.response.headers.get("etag") ?? "";
      if (!silent) setStatus(`${apps.length} installed app(s)`);
      renderApps();
      renderComposition();
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
      const head = element("tr");
      for (const label of ["APP", "VERSION", "STATE", "GRANT", "ACTIONS"]) {
        head.append(element("th", "", label));
      }
      table.append(head);
      for (const app of apps) {
        const row = element("tr");
        const id = element("td");
        id.append(element("div", "mgr-mono", app.id));
        if (app.alias) id.append(element("div", "mgr-mono", `alias ${app.alias}`));
        row.append(id);

        row.append(element("td", "mgr-mono", app.version));

        const state = element("td");
        state.append(
          badge(app.enabled ? "enabled" : "disabled", app.enabled ? "ok" : "warn"),
          document.createTextNode(" "),
          badge(app.tier === "system" ? "system" : "third-party"),
        );
        row.append(state);

        const grant = element("td");
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

        const actions = element("td");
        const tools = element("div", "mgr-actions");
        const toggle = button(
          "mgr-btn",
          app.enabled ? "Disable" : "Enable",
          app.enabled ? "Disable this app" : "Enable this app",
        );
        toggle.addEventListener("click", () => void patch(app, !app.enabled));
        const approve = button("mgr-btn primary", "Approve", "Approve a grant");
        approve.addEventListener("click", () => {
          pendingApprove = pendingApprove === app.id ? null : app.id;
          renderApps();
        });
        const remove = button("mgr-btn danger", "Remove", "Remove this app");
        remove.addEventListener("click", () => void remove(app));
        tools.append(toggle, approve);
        if (app.grant_state !== "none") {
          const revoke = button("mgr-btn danger", "Revoke", "Revoke the grant");
          revoke.addEventListener("click", () => void revoke(app));
          tools.append(revoke);
        }
        tools.append(remove);
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
    approveRoot.append(element("strong", "mgr-mono", `Approve ${app.id}`));
    const chosen: HTMLInputElement[] = [];
    for (const risk of RISKS) {
      const label = element("label", "mgr-check");
      const input = document.createElement("input");
      input.type = "checkbox";
      input.value = risk;
      if (app.grant?.accepted.includes(risk)) input.checked = true;
      chosen.push(input);
      label.append(input, document.createTextNode(` ${risk}`));
      approveRoot.append(label);
    }
    const confirm = button("mgr-btn primary", "Approve");
    confirm.addEventListener("click", () => {
      void approve(
        app,
        chosen.filter((input) => input.checked).map((input) => input.value as Risk),
      );
    });
    const cancel = button("mgr-btn", "Cancel");
    cancel.addEventListener("click", () => {
      pendingApprove = null;
      renderApps();
    });
    approveRoot.append(confirm, cancel);
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

  void refresh();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
