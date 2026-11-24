import { applyBody, call, watchLogs } from "./api";
import { styles } from "./styles";
import type {
  AccessEvent,
  FrameContext,
  ProxyConfig,
  ProxyHost,
  ProxyListener,
  ProxyRoute,
  ProxyStatus,
  ProxyUpstream,
} from "./types";

type Section = "listeners" | "hosts" | "upstreams" | "log";

let pending: ((params: unknown) => void) | null = null;

/// The modals hand a draft piece back through `openView` params; the panel
/// calls this export on the already mounted view.
export function onParams(params: unknown): void {
  pending?.(params);
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function escape(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function stamp(seconds: number): string {
  return seconds ? new Date(seconds * 1000).toLocaleTimeString() : "—";
}

function option(value: string, label: string, selected: boolean): string {
  return `<option value="${escape(value)}"${selected ? " selected" : ""}>${escape(label)}</option>`;
}

function uniqueId(existing: string[], base: string): string {
  if (!base) base = "route";
  if (!existing.includes(base)) return base;
  let index = 2;
  while (existing.includes(`${base}-${index}`)) index += 1;
  return `${base}-${index}`;
}

function field(name: string, value: string): string {
  return `<span class="px-field"><span class="px-field-name">${name}</span><span>${escape(
    value,
  )}</span></span>`;
}

export function mount(root: HTMLElement, context: FrameContext): () => void {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const canEdit = context.panel?.slots?.includes("panel.modal") === true;
  const hasFab = context.panel?.slots?.includes("panel.fab") === true;

  const box = document.createElement("div");
  box.className = "px";
  box.dataset.testid = "proxy-view";
  box.innerHTML = `
    <div class="px-bar">
      <div class="px-tabs" role="tablist">
        <button type="button" role="tab" data-section="listeners" data-testid="proxy-tab-listeners">Listeners</button>
        <button type="button" role="tab" data-section="hosts" data-testid="proxy-tab-hosts">Hosts &amp; routes</button>
        <button type="button" role="tab" data-section="upstreams" data-testid="proxy-tab-upstreams">Upstreams</button>
        <button type="button" role="tab" data-section="log" data-testid="proxy-tab-log">Access log</button>
      </div>
      <span class="grow"></span>
      <span class="px-badge" data-running>—</span>
      <button type="button" data-reload-certs data-testid="proxy-reload-certs" title="Re-read the published certificates">Reload certs</button>
      <button type="button" data-refresh data-testid="proxy-refresh">Refresh</button>
      <button type="button" class="primary" data-apply data-testid="proxy-apply">Apply</button>
    </div>
    <div class="px-banner" data-banner data-testid="proxy-banner" hidden></div>
    <div class="px-section" data-body data-testid="proxy-body"></div>
    <p class="px-status" role="status" data-status data-testid="proxy-status"></p>
  `;
  root.append(box);

  const body = box.querySelector("[data-body]") as HTMLElement;
  const banner = box.querySelector("[data-banner]") as HTMLElement;
  const statusLine = box.querySelector("[data-status]") as HTMLElement;
  const running = box.querySelector("[data-running]") as HTMLElement;

  let config: ProxyConfig | null = null;
  let draft: ProxyConfig | null = null;
  let certificates: string[] = [];
  let status: ProxyStatus | null = null;
  let section: Section = "listeners";
  let events: AccessEvent[] = [];
  let controller: AbortController | null = null;
  let disposed = false;
  /// A modal just handed a draft piece; the panel's automatic `refresh` action
  /// after closing it must not reload the server document over that draft.
  let pendingApply = false;

  const setStatus = (message: string, bad = false): void => {
    statusLine.textContent = message;
    statusLine.className = bad ? "px-status bad" : "px-status";
  };

  const showStatus = (): void => {
    if (!status) {
      running.textContent = "—";
      running.className = "px-badge";
      banner.hidden = true;
      return;
    }
    running.textContent = status.running ? "running" : "stopped";
    running.className = status.running ? "px-badge ok" : "px-badge bad";
    const handshakes = status["handshake-errors"] ?? 0;
    running.title = `${status.listeners.join(", ")}${
      handshakes ? ` · ${handshakes} TLS handshake error(s)` : ""
    }`;
    banner.hidden = !status["last-error"];
    banner.textContent = status["last-error"] ?? "";
  };

  const requireDraft = (): ProxyConfig | null => {
    if (!draft) {
      setStatus("The document has not loaded yet.", true);
      return null;
    }
    return draft;
  };

  async function refreshStatus(): Promise<void> {
    try {
      status = (await call(context, "/status")) as ProxyStatus;
      if (!disposed) showStatus();
    } catch {
      // The banner keeps the last error; a failed status read is not fatal.
    }
  }

  async function load(): Promise<void> {
    setStatus("Reading the proxy document…");
    try {
      const [next, state, ids] = await Promise.all([
        call(context, "/config") as Promise<ProxyConfig>,
        call(context, "/status") as Promise<ProxyStatus>,
        call(context, "/certificates") as Promise<string[]>,
      ]);
      if (disposed) return;
      config = next;
      draft = clone(next);
      status = state;
      certificates = Array.isArray(ids) ? ids : [];
      showStatus();
      render();
      setStatus(`Loaded v${next.version}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function apply(): Promise<void> {
    const document = requireDraft();
    if (!document || !config) return;
    setStatus("Validating and applying the whole document…");
    try {
      const next = (await call(context, "/config", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(applyBody(document, config.version)),
      })) as ProxyConfig;
      if (disposed) return;
      config = next;
      draft = clone(next);
      await refreshStatus();
      if (disposed) return;
      render();
      setStatus(`Applied v${next.version}`);
    } catch (error) {
      if (disposed) return;
      setStatus(String(error), true);
      await refreshStatus();
    }
  }

  async function reloadCertificates(): Promise<void> {
    setStatus("Re-reading the published certificates…");
    try {
      status = (await call(context, "/certificates/reload", {
        method: "POST",
      })) as ProxyStatus;
      if (disposed) return;
      showStatus();
      setStatus("Certificates reloaded");
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function testUpstream(id: string): Promise<void> {
    setStatus(`Testing ${id}…`);
    try {
      const result = await call(context, "/upstreams/test", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id }),
      });
      if (disposed) return;
      const state = result.ok ? "ok" : "failed";
      setStatus(
        `${id}: ${state} · ${result.status ?? "no status"} · ${result["latency-ms"]} ms${
          result.error ? ` · ${result.error}` : ""
        }`,
        !result.ok,
      );
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  function stopLogs(): void {
    controller?.abort();
    controller = null;
  }

  function startLogs(): void {
    stopLogs();
    const abort = new AbortController();
    controller = abort;
    setStatus("Following the access log…");
    watchLogs(context, abort.signal, (event) => {
      if (disposed || section !== "log") return;
      events.push(event);
      if (events.length > 200) events.splice(0, events.length - 200);
      appendEvent(event);
    })
      .then(() => {
        if (!disposed && !abort.signal.aborted) {
          setStatus("The access log ended.", true);
        }
      })
      .catch((error) => {
        if (!disposed && !abort.signal.aborted) setStatus(String(error), true);
      });
  }

  function appendEvent(event: AccessEvent): void {
    const list = body.querySelector("[data-log]");
    if (!list) return;
    list.insertAdjacentHTML("beforeend", eventRow(event));
    list.scrollTop = list.scrollHeight;
    const count = body.querySelector("[data-log-count]");
    if (count) count.textContent = String(events.length);
  }

  function eventRow(event: AccessEvent): string {
    const bad = event.status >= 500 ? " bad" : "";
    return `<div class="px-event">
      <span>${stamp(event.at)}</span>
      <span>${escape(event.host)}</span>
      <span>${escape(event.method)}</span>
      <span>${escape(event.path)}</span>
      <span class="${bad.trim()}">${event.status}</span>
      <span>${event.bytes}</span>
      <span>${escape(event.upstream || "—")}</span>
    </div>`;
  }

  function renderTabs(): void {
    box
      .querySelectorAll<HTMLButtonElement>("[data-section]")
      .forEach((button) => {
        button.classList.toggle("active", button.dataset.section === section);
      });
  }

  function render(): void {
    renderTabs();
    if (!draft) {
      body.innerHTML = `<div class="px-empty">Reading the document…</div>`;
      return;
    }
    const leavingLog = section !== "log";
    if (leavingLog) stopLogs();
    if (section === "listeners") renderListeners();
    else if (section === "upstreams") renderUpstreams();
    else if (section === "hosts") renderHosts();
    else renderLogs();
  }

  function formNote(): string {
    return canEdit
      ? ""
      : `<div class="px-note">This panel has no modal slot; the document is read-only here.</div>`;
  }

  function renderListeners(): void {
    const document = requireDraft();
    if (!document) return;
    const rows = document.listeners
      .map(
        (listener, index) => `<div class="px-grid hosts" data-testid="proxy-listener" data-listener-id="${escape(listener.id)}">
          ${field("Id", listener.id)}
          ${field("Bind", listener.bind)}
          ${field("Kind", listener.tls ? "TLS" : "HTTP")}
          ${field("Default host", listener["default-host"] ?? "(none)")}
          ${field("Trust", listener.forwarded ? "forwarded" : "—")}
          ${field("", "")}
          <span class="px-field"><span class="px-field-name">Actions</span><span class="px-actions">
            ${
              canEdit
                ? `<button type="button" data-edit-listener="${index}" data-testid="proxy-edit-listener">Edit</button>`
                : ""
            }
          </span></span>
        </div>`,
      )
      .join("");
    body.innerHTML = `${formNote()}<div class="px-note">The binds are frozen in the manifest; the document maps hosts onto them and cannot open another port.</div>
      <div class="px-grid hosts head"><span>Id</span><span>Bind</span><span>Kind</span><span>Default host</span><span>Trust</span><span></span><span></span></div>
      ${rows || `<div class="px-empty">No listener in the document.</div>`}`;

    body.querySelectorAll<HTMLButtonElement>("[data-edit-listener]").forEach((button) => {
      button.addEventListener("click", () => {
        const listener = draft?.listeners[Number(button.dataset.editListener)];
        if (!listener || !draft) return;
        void openForm("proxy-listener-modal", {
          listener,
          hosts: draft.hosts.map((host) => ({
            id: host.id,
            names: host.names,
          })),
        });
      });
    });
  }

  function renderUpstreams(): void {
    const document = requireDraft();
    if (!document) return;
    const rows = document.upstreams
      .map(
        (upstream, index) => `<div class="px-grid" data-testid="proxy-upstream" data-upstream-id="${escape(upstream.id)}">
          ${field("Id", upstream.id)}
          <span class="px-field"><span class="px-field-name">URL</span><span class="px-mono">${escape(
            upstream.url,
          )}</span></span>
          ${field("Connect ms", String(upstream["connect-timeout-ms"]))}
          ${field("Read ms", String(upstream["read-timeout-ms"]))}
          ${field("TLS", upstream["tls-verify"] ? "verify" : "no verify")}
          <span class="px-field"><span class="px-field-name">Actions</span><span class="px-actions">
            <button type="button" data-test-upstream="${escape(upstream.id)}" data-testid="proxy-test-upstream">Test</button>
            ${
              canEdit
                ? `<button type="button" data-edit-upstream="${index}" data-testid="proxy-edit-upstream">Edit</button>`
                : ""
            }
            <button type="button" class="danger" data-remove-upstream="${index}" data-testid="proxy-remove-upstream">Remove</button>
          </span></span>
        </div>`,
      )
      .join("");
    body.innerHTML = `${formNote()}<div class="px-bar"><span class="px-note">Upstreams are http(s) origins without a path.</span><span class="grow"></span>${
      canEdit && !hasFab
        ? `<button type="button" data-add-upstream data-testid="proxy-add-upstream">Add upstream</button>`
        : ""
    }</div>
      <div class="px-grid head"><span>Id</span><span>URL</span><span>Connect ms</span><span>Read ms</span><span>TLS</span><span></span></div>
      ${rows || `<div class="px-empty">No upstream in the document.</div>`}`;

    body.querySelectorAll<HTMLButtonElement>("[data-test-upstream]").forEach((button) => {
      button.addEventListener("click", () => {
        void testUpstream(button.dataset.testUpstream ?? "");
      });
    });
    body.querySelectorAll<HTMLButtonElement>("[data-remove-upstream]").forEach((button) => {
      button.addEventListener("click", () => {
        draft?.upstreams.splice(Number(button.dataset.removeUpstream), 1);
        render();
      });
    });
    body.querySelectorAll<HTMLButtonElement>("[data-edit-upstream]").forEach((button) => {
      button.addEventListener("click", () => {
        const upstream = draft?.upstreams[Number(button.dataset.editUpstream)];
        if (upstream) void openForm("proxy-upstream-modal", { upstream });
      });
    });
    body.querySelector("[data-add-upstream]")?.addEventListener("click", () => {
      void openForm("proxy-upstream-modal");
    });
  }

  function renderHosts(): void {
    const document = requireDraft();
    if (!document) return;
    const cards = document.hosts
      .map((host, index) => {
        const routes = host.routes
          .map(
            (route, routeIndex) => `<div class="px-grid hosts" data-testid="proxy-route" data-route-id="${escape(route.id)}">
              ${field("Match", route.match)}
              ${field("Target", route.redirect ? `→ ${route.redirect}` : route.upstream ?? "—")}
              ${field(
                "Flags",
                `${route["strip-prefix"] ? "strip" : ""}${route.websocket ? " ws" : ""}`,
              )}
              ${field("Limit", route["body-limit"]?.toString() ?? "")}
              <span class="px-field"></span>
              <span class="px-field"><span class="px-field-name">Actions</span><span class="px-actions">
                ${
                  canEdit
                    ? `<button type="button" data-edit-route="${index}:${routeIndex}" data-testid="proxy-edit-route">Edit</button>`
                    : ""
                }
                <button type="button" class="danger" data-remove-route="${index}:${routeIndex}" data-testid="proxy-remove-route">Remove</button>
              </span></span>
              <span class="px-field"></span>
            </div>`,
          )
          .join("");
        return `<div class="px-card" data-testid="proxy-host" data-host-id="${escape(host.id)}">
          <div class="px-card-head">
            <strong>${escape(host.id)}</strong>
            <span class="grow"></span>
            ${
              canEdit
                ? `<button type="button" data-edit-host="${index}" data-testid="proxy-edit-host">Edit</button>`
                : ""
            }
            <button type="button" class="danger" data-remove-host="${index}" data-testid="proxy-remove-host">Remove host</button>
          </div>
          <div class="px-grid hosts">
            ${field("Names", host.names.join(", "))}
            ${field("Certificate", host.certificate ?? "(none)")}
            ${field("ACME", host.acme ? "yes" : "no")}
            ${field("HTTPS", host["redirect-https"] ? "redirect" : "no")}
            ${field("HSTS", host.hsts ? "yes" : "no")}
            <span class="px-field"></span>
            <span class="px-field"></span>
          </div>
          <div class="px-grid hosts head"><span>Match</span><span>Target</span><span>Flags</span><span>Limit</span><span></span><span></span><span></span></div>
          ${routes || `<div class="px-empty">No route on this host.</div>`}
          ${
            canEdit
              ? `<div class="px-bar"><span class="grow"></span><button type="button" data-add-route="${index}" data-testid="proxy-add-route">Add route</button></div>`
              : ""
          }
        </div>`;
      })
      .join("");
    body.innerHTML = `${formNote()}<div class="px-bar"><span class="px-note">A host is one certificate and one route table; an unmatched name goes to the listener default.</span><span class="grow"></span>${
      canEdit && !hasFab
        ? `<button type="button" data-add-host data-testid="proxy-add-host">Add host</button>`
        : ""
    }</div>
      ${cards || `<div class="px-empty">No host in the document.</div>`}`;

    body.querySelectorAll<HTMLButtonElement>("[data-remove-host]").forEach((button) => {
      button.addEventListener("click", () => {
        draft?.hosts.splice(Number(button.dataset.removeHost), 1);
        render();
      });
    });
    body.querySelectorAll<HTMLButtonElement>("[data-edit-host]").forEach((button) => {
      button.addEventListener("click", () => {
        const host = draft?.hosts[Number(button.dataset.editHost)];
        if (host) void openForm("proxy-host-modal", { host });
      });
    });
    body.querySelectorAll<HTMLButtonElement>("[data-remove-route]").forEach((button) => {
      button.addEventListener("click", () => {
        const [hostIndex, routeIndex] = String(button.dataset.removeRoute)
          .split(":")
          .map(Number);
        draft?.hosts[hostIndex]?.routes.splice(routeIndex, 1);
        render();
      });
    });
    body.querySelectorAll<HTMLButtonElement>("[data-edit-route]").forEach((button) => {
      button.addEventListener("click", () => {
        const [hostIndex, routeIndex] = String(button.dataset.editRoute)
          .split(":")
          .map(Number);
        const host = draft?.hosts[hostIndex];
        const route = host?.routes[routeIndex];
        if (!host || !route || !draft) return;
        void openForm("proxy-route-modal", {
          hostId: host.id,
          route,
          upstreams: draft.upstreams.map((upstream) => upstream.id),
        });
      });
    });
    body.querySelectorAll<HTMLButtonElement>("[data-add-route]").forEach((button) => {
      button.addEventListener("click", () => {
        const host = draft?.hosts[Number(button.dataset.addRoute)];
        if (!host || !draft) return;
        void openForm("proxy-route-modal", {
          hostId: host.id,
          upstreams: draft.upstreams.map((upstream) => upstream.id),
        });
      });
    });
    body.querySelector("[data-add-host]")?.addEventListener("click", () => {
      void openForm("proxy-host-modal");
    });
  }

  function renderLogs(): void {
    body.innerHTML = `<div class="px-bar"><span class="px-note">One event per request; the stream drops under budget and reports the count.</span><span class="grow"></span><span class="px-badge" data-log-count>${events.length} events</span><span class="px-badge">dropped ${status?.dropped ?? 0}</span>${
      (status?.["handshake-errors"] ?? 0) > 0
        ? `<span class="px-badge bad">handshake errors ${status?.["handshake-errors"]}</span>`
        : ""
    }<button type="button" data-clear-log>Clear</button></div>
      <div class="px-log" data-log data-testid="proxy-log">${events.map(eventRow).join("")}</div>`;
    body.querySelector("[data-clear-log]")?.addEventListener("click", () => {
      events = [];
      render();
    });
    startLogs();
  }

  async function openForm(tag: string, params?: unknown): Promise<void> {
    if (!canEdit || !context.openModal) {
      setStatus("This panel has no modal slot; the document is read-only here.", true);
      return;
    }
    const opened = await context.openModal({ tag, params });
    if (!opened) setStatus("The panel refused to open the form.", true);
  }

  function receiveHost(host: ProxyHost): void {
    if (!draft) return;
    const index = draft.hosts.findIndex((candidate) => candidate.id === host.id);
    if (index >= 0) draft.hosts[index] = host;
    else draft.hosts.push(host);
    section = "hosts";
    render();
    setStatus(`Host ${host.id} updated in the draft; apply to publish it.`);
  }

  function receiveUpstream(upstream: ProxyUpstream): void {
    if (!draft) return;
    const index = draft.upstreams.findIndex(
      (candidate) => candidate.id === upstream.id,
    );
    if (index >= 0) draft.upstreams[index] = upstream;
    else draft.upstreams.push(upstream);
    section = "upstreams";
    render();
    setStatus(`Upstream ${upstream.id} updated in the draft.`);
  }

  function receiveRoute(payload: {
    hostId?: string;
    route?: ProxyRoute;
  }): void {
    if (!draft || !payload.hostId || !payload.route) return;
    const host = draft.hosts.find((candidate) => candidate.id === payload.hostId);
    if (!host) return;
    const route = { ...payload.route };
    if (!route.id) {
      route.id = uniqueId(
        host.routes.map((candidate) => candidate.id),
        "route",
      );
    }
    const index = host.routes.findIndex((candidate) => candidate.id === route.id);
    if (index >= 0) host.routes[index] = route;
    else host.routes.push(route);
    section = "hosts";
    render();
    setStatus(`Route ${route.match} updated in the draft.`);
  }

  function receiveListener(listener: ProxyListener): void {
    if (!draft) return;
    const index = draft.listeners.findIndex(
      (candidate) => candidate.id === listener.id,
    );
    if (index >= 0) draft.listeners[index] = listener;
    section = "listeners";
    render();
    setStatus(`Listener ${listener.id} updated in the draft.`);
  }

  function applyParams(params: unknown): void {
    if (!params || typeof params !== "object") return;
    const value = params as Record<string, unknown>;
    if (value.addHost) receiveHost(value.addHost as ProxyHost);
    else if (value.upsertHost) receiveHost(value.upsertHost as ProxyHost);
    else if (value.upsertUpstream) {
      receiveUpstream(value.upsertUpstream as ProxyUpstream);
    } else if (value.upsertRoute) {
      receiveRoute(
        value.upsertRoute as { hostId?: string; route?: ProxyRoute },
      );
    } else if (value.upsertListener) {
      receiveListener(value.upsertListener as ProxyListener);
    }
    pendingApply = true;
  }

  pending = applyParams;

  box.querySelectorAll<HTMLButtonElement>("[data-section]").forEach((button) => {
    button.addEventListener("click", () => {
      section = (button.dataset.section as Section) ?? "listeners";
      render();
    });
  });
  box.querySelector("[data-refresh]")?.addEventListener("click", () => {
    void load();
  });
  box.querySelector("[data-apply]")?.addEventListener("click", () => {
    void apply();
  });
  box.querySelector("[data-reload-certs]")?.addEventListener("click", () => {
    void reloadCertificates();
  });

  const offAction = context.onAction?.((tag) => {
    if (tag === "apply") void apply();
    else if (tag === "refresh") {
      if (pendingApply) {
        pendingApply = false;
        return;
      }
      void load();
    } else if (tag === "add-host") void openForm("proxy-host-modal");
    else if (tag === "add-upstream") void openForm("proxy-upstream-modal");
  });
  const offVisibility = context.onVisibility?.((visible) => {
    if (!visible) {
      stopLogs();
      return;
    }
    if (section === "log") startLogs();
    else void refreshStatus();
  });
  // A modal hands its piece through `openView` params. When the view was not
  // mounted, the panel opens it with those params instead of calling onParams;
  // read them after the document loads so the draft exists.
  const initial = context.params;
  void load().then(() => {
    if (!disposed && initial) applyParams(initial);
  });

  return () => {
    disposed = true;
    pending = null;
    offAction?.();
    offVisibility?.();
    stopLogs();
    root.replaceChildren();
  };
}
