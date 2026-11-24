import { applyBody, call, watchLogs } from "./api";
import { styles } from "./styles";
import type {
  AccessEvent,
  FrameContext,
  ProxyConfig,
  ProxyHost,
  ProxyRoute,
  ProxyStatus,
} from "./types";

type Section = "listeners" | "hosts" | "upstreams" | "log";

let pendingHost: ((host: ProxyHost) => void) | null = null;

/// The Add-host modal hands the new host back through `openView` params; the
/// panel calls this export on the already mounted view.
export function onParams(params: unknown): void {
  if (!params || typeof params !== "object") return;
  const host = (params as { addHost?: ProxyHost }).addHost;
  if (host) pendingHost?.(host);
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
  if (!existing.includes(base)) return base;
  let index = 2;
  while (existing.includes(`${base}-${index}`)) index += 1;
  return `${base}-${index}`;
}

export function mount(root: HTMLElement, context: FrameContext): () => void {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

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
        button.classList.toggle(
          "active",
          button.dataset.section === section,
        );
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

  function renderListeners(): void {
    const document = requireDraft();
    if (!document) return;
    const rows = document.listeners
      .map((listener, index) => {
        const hosts = document.hosts
          .map((host) =>
            option(
              host.id,
              `${host.id} (${host.names.join(", ")})`,
              listener["default-host"] === host.id,
            ),
          )
          .join("");
        return `<div class="px-grid hosts" data-testid="proxy-listener" data-listener-id="${escape(listener.id)}">
          <span class="px-field"><span class="px-field-name">Id</span><span class="px-mono">${escape(listener.id)}</span></span>
          <span class="px-field"><span class="px-field-name">Bind</span><span class="px-mono">${escape(listener.bind)}</span></span>
          <span class="px-field"><span class="px-field-name">Kind</span><span>${listener.tls ? "TLS" : "HTTP"}</span></span>
          <span class="px-field"><span class="px-field-name">Default host</span><select data-default-host="${index}">
            ${option("", "(none)", listener["default-host"] === null)}
            ${hosts}
          </select></span>
          <span class="px-field"><span class="px-field-name">Trust</span><label class="px-check"><input type="checkbox" data-forwarded="${index}"${
            listener.forwarded ? " checked" : ""
          } /> forwarded</label></span>
          <span class="px-field"></span>
        </div>`;
      })
      .join("");
    body.innerHTML = `<div class="px-note">The binds are frozen in the manifest; the document maps hosts onto them and cannot open another port.</div>
      <div class="px-grid hosts head"><span>Id</span><span>Bind</span><span>Kind</span><span>Default host</span><span>Trust</span><span></span></div>
      ${rows || `<div class="px-empty">No listener in the document.</div>`}`;

    body
      .querySelectorAll<HTMLSelectElement>("[data-default-host]")
      .forEach((select) => {
        select.addEventListener("change", () => {
          const listener = draft?.listeners[Number(select.dataset.defaultHost)];
          if (!listener) return;
          listener["default-host"] = select.value || null;
        });
      });
    body
      .querySelectorAll<HTMLInputElement>("[data-forwarded]")
      .forEach((input) => {
        input.addEventListener("change", () => {
          const listener = draft?.listeners[Number(input.dataset.forwarded)];
          if (!listener) return;
          listener.forwarded = input.checked;
        });
      });
  }

  function renderUpstreams(): void {
    const document = requireDraft();
    if (!document) return;
    const rows = document.upstreams
      .map(
        (upstream, index) => `<div class="px-grid" data-testid="proxy-upstream" data-upstream-id="${escape(upstream.id)}">
          <span class="px-field"><span class="px-field-name">Id</span><span class="px-mono">${escape(upstream.id)}</span></span>
          <span class="px-field"><span class="px-field-name">URL</span><input type="text" data-upstream-url="${index}" value="${escape(upstream.url)}" /></span>
          <span class="px-field"><span class="px-field-name">Connect ms</span><input type="number" min="1" max="300000" data-upstream-connect="${index}" value="${upstream["connect-timeout-ms"]}" /></span>
          <span class="px-field"><span class="px-field-name">Read ms</span><input type="number" min="1" max="300000" data-upstream-read="${index}" value="${upstream["read-timeout-ms"]}" /></span>
          <span class="px-field"><span class="px-field-name">TLS</span><label class="px-check"><input type="checkbox" data-upstream-verify="${index}"${
            upstream["tls-verify"] ? " checked" : ""
          } /> verify</label></span>
          <span class="px-field"><span class="px-field-name">Actions</span><span class="px-actions">
            <button type="button" data-test-upstream="${escape(upstream.id)}">Test</button>
            <button type="button" class="danger" data-remove-upstream="${index}">Remove</button>
          </span></span>
        </div>`,
      )
      .join("");
    body.innerHTML = `<div class="px-grid head"><span>Id</span><span>URL</span><span>Connect ms</span><span>Read ms</span><span>TLS</span><span></span></div>
      ${rows || `<div class="px-empty">No upstream in the document.</div>`}
      <form class="px-form" data-add-upstream>
        <input type="text" name="id" placeholder="upstream id" required />
        <input type="text" name="url" placeholder="http://127.0.0.1:3000" required />
        <button type="submit">Add upstream</button>
      </form>`;

    body
      .querySelectorAll<HTMLInputElement>("[data-upstream-url]")
      .forEach((input) => {
        input.addEventListener("input", () => {
          const upstream = draft?.upstreams[Number(input.dataset.upstreamUrl)];
          if (upstream) upstream.url = input.value;
        });
      });
    body
      .querySelectorAll<HTMLInputElement>("[data-upstream-connect]")
      .forEach((input) => {
        input.addEventListener("input", () => {
          const upstream = draft?.upstreams[Number(input.dataset.upstreamConnect)];
          if (upstream) upstream["connect-timeout-ms"] = Number(input.value) || 0;
        });
      });
    body
      .querySelectorAll<HTMLInputElement>("[data-upstream-read]")
      .forEach((input) => {
        input.addEventListener("input", () => {
          const upstream = draft?.upstreams[Number(input.dataset.upstreamRead)];
          if (upstream) upstream["read-timeout-ms"] = Number(input.value) || 0;
        });
      });
    body
      .querySelectorAll<HTMLInputElement>("[data-upstream-verify]")
      .forEach((input) => {
        input.addEventListener("change", () => {
          const upstream = draft?.upstreams[Number(input.dataset.upstreamVerify)];
          if (upstream) upstream["tls-verify"] = input.checked;
        });
      });
    body.querySelectorAll<HTMLButtonElement>("[data-remove-upstream]").forEach((button) => {
      button.addEventListener("click", () => {
        draft?.upstreams.splice(Number(button.dataset.removeUpstream), 1);
        render();
      });
    });
    body.querySelectorAll<HTMLButtonElement>("[data-test-upstream]").forEach((button) => {
      button.addEventListener("click", () => {
        void testUpstream(button.dataset.testUpstream ?? "");
      });
    });
    const form = body.querySelector<HTMLFormElement>("[data-add-upstream]");
    form?.addEventListener("submit", (event) => {
      event.preventDefault();
      const data = new FormData(form);
      const id = String(data.get("id") ?? "").trim();
      const url = String(data.get("url") ?? "").trim();
      if (!document || !id || !url) return;
      document.upstreams.push({
        id,
        url,
        "connect-timeout-ms": 2_000,
        "read-timeout-ms": 5_000,
        "tls-verify": true,
      });
      render();
    });
  }

  function renderHosts(): void {
    const document = requireDraft();
    if (!document) return;
    const cards = document.hosts
      .map((host, index) => {
        const options = [
          option("", "(none)", host.certificate === null),
          ...certificates.map((id) => option(id, id, host.certificate === id)),
        ].join("");
        const routes = host.routes
          .map(
            (route, routeIndex) => `<div class="px-grid hosts" data-testid="proxy-route" data-route-id="${escape(route.id)}">
              <span class="px-field"><span class="px-field-name">Match</span><span class="px-mono">${escape(route.match)}</span></span>
              <span class="px-field"><span class="px-field-name">Target</span><span class="px-mono">${escape(
                route.redirect ? `→ ${route.redirect}` : route.upstream ?? "—",
              )}</span></span>
              <span class="px-field"><span class="px-field-name">Flags</span><span>${route["strip-prefix"] ? "strip" : ""}${
                route.websocket ? " ws" : ""
              }</span></span>
              <span class="px-field"><span class="px-field-name">Limit</span><span>${route["body-limit"] ?? ""}</span></span>
              <span class="px-field"></span>
              <span class="px-field"><span class="px-field-name">Actions</span><button type="button" class="danger" data-remove-route="${index}:${routeIndex}">Remove</button></span>
              <span class="px-field"></span>
            </div>`,
          )
          .join("");
        const upstreamOptions = [
          option("", "(none)", false),
          ...document.upstreams.map((upstream) =>
            option(upstream.id, upstream.id, false),
          ),
        ].join("");
        return `<div class="px-card" data-testid="proxy-host" data-host-id="${escape(host.id)}">
          <div class="px-card-head">
            <strong>${escape(host.id)}</strong>
            <span class="grow"></span>
            <button type="button" class="danger" data-remove-host="${index}">Remove host</button>
          </div>
          <div class="px-grid hosts">
            <span class="px-field"><span class="px-field-name">Names</span><input type="text" data-host-names="${index}" value="${escape(host.names.join(", "))}" /></span>
            <span class="px-field"><span class="px-field-name">Certificate</span><select data-host-certificate="${index}">${options}</select></span>
            <span class="px-field"><span class="px-field-name">ACME</span><label class="px-check"><input type="checkbox" data-host-acme="${index}"${
              host.acme ? " checked" : ""
            } /> acme</label></span>
            <span class="px-field"><span class="px-field-name">HTTPS</span><label class="px-check"><input type="checkbox" data-host-redirect="${index}"${
              host["redirect-https"] ? " checked" : ""
            } /> redirect</label></span>
            <span class="px-field"><span class="px-field-name">HSTS</span><label class="px-check"><input type="checkbox" data-host-hsts="${index}"${
              host.hsts ? " checked" : ""
            } /> hsts</label></span>
            <span class="px-field"></span>
            <span class="px-field"></span>
          </div>
          <div class="px-grid hosts head"><span>Match</span><span>Target</span><span>Flags</span><span>Limit</span><span></span><span></span><span></span></div>
          ${routes || `<div class="px-empty">No route on this host.</div>`}
          <form class="px-form" data-add-route="${index}">
            <input type="text" name="match" placeholder="= /health or /" required />
            <select name="upstream">${upstreamOptions}</select>
            <input type="text" name="redirect" placeholder="optional redirect" />
            <label class="px-check"><input type="checkbox" name="strip" /> strip</label>
            <label class="px-check"><input type="checkbox" name="websocket" /> ws</label>
            <input type="number" name="limit" min="1" placeholder="body limit" />
            <button type="submit">Add route</button>
          </form>
        </div>`;
      })
      .join("");
    body.innerHTML = `<div class="px-bar"><span class="px-note">A host is one certificate and one route table; an unmatched name goes to the listener default.</span><span class="grow"></span><button type="button" data-add-host data-testid="proxy-add-host">Add host</button></div>
      ${cards || `<div class="px-empty">No host in the document.</div>`}`;

    body
      .querySelectorAll<HTMLInputElement>("[data-host-names]")
      .forEach((input) => {
        input.addEventListener("input", () => {
          const host = draft?.hosts[Number(input.dataset.hostNames)];
          if (host) host.names = input.value.split(",").map((name) => name.trim()).filter(Boolean);
        });
      });
    body
      .querySelectorAll<HTMLSelectElement>("[data-host-certificate]")
      .forEach((select) => {
        select.addEventListener("change", () => {
          const host = draft?.hosts[Number(select.dataset.hostCertificate)];
          if (host) host.certificate = select.value || null;
        });
      });
    for (const [selector, key, field] of [
      ["[data-host-acme]", "hostAcme", "acme"],
      ["[data-host-redirect]", "hostRedirect", "redirect-https"],
      ["[data-host-hsts]", "hostHsts", "hsts"],
    ] as const) {
      body.querySelectorAll<HTMLInputElement>(selector).forEach((input) => {
        input.addEventListener("change", () => {
          const host = draft?.hosts[Number(input.dataset[key])];
          if (host) (host as any)[field] = input.checked;
        });
      });
    }
    body.querySelectorAll<HTMLButtonElement>("[data-remove-host]").forEach((button) => {
      button.addEventListener("click", () => {
        draft?.hosts.splice(Number(button.dataset.removeHost), 1);
        render();
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
    body.querySelectorAll<HTMLFormElement>("[data-add-route]").forEach((form) => {
      form.addEventListener("submit", (event) => {
        event.preventDefault();
        const index = Number(form.dataset.addRoute);
        const host = draft?.hosts[index];
        if (!host) return;
        const data = new FormData(form);
        const redirect = String(data.get("redirect") ?? "").trim();
        const upstream = String(data.get("upstream") ?? "").trim();
        const limit = String(data.get("limit") ?? "").trim();
        const route: ProxyRoute = {
          id: uniqueId(
            host.routes.map((route) => route.id),
            "route",
          ),
          match: String(data.get("match") ?? "").trim(),
          upstream: redirect ? null : upstream || null,
          "strip-prefix": data.get("strip") === "on",
          websocket: data.get("websocket") === "on",
          redirect: redirect || null,
          headers: [],
          "body-limit": limit ? Number(limit) : null,
        };
        host.routes.push(route);
        render();
      });
    });
    body.querySelector("[data-add-host]")?.addEventListener("click", () => {
      void addHost();
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

  async function addHost(): Promise<void> {
    if (!context.openModal) {
      setStatus("This panel has no modal slot; add the host in the document directly.", true);
      return;
    }
    const opened = await context.openModal({ tag: "proxy-host-modal" });
    if (!opened) setStatus("The panel refused to open the add-host modal.", true);
  }

  function receiveHost(host: ProxyHost): void {
    if (!draft) return;
    if (draft.hosts.some((candidate) => candidate.id === host.id)) {
      setStatus(`A host called ${host.id} already exists.`, true);
      return;
    }
    draft.hosts.push(host);
    section = "hosts";
    render();
    setStatus(`Added ${host.id} to the draft; apply to publish it.`);
  }

  pendingHost = receiveHost;

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
    else if (tag === "refresh") void load();
    else if (tag === "add-host") void addHost();
  });
  const offVisibility = context.onVisibility?.((visible) => {
    if (!visible) {
      stopLogs();
      return;
    }
    if (section === "log") startLogs();
    else void refreshStatus();
  });
  // A modal hands the host through `openView` params. When the view was not
  // mounted, the panel opens it with those params instead of calling onParams;
  // read them after the document loads so the draft exists.
  const initial = (context.params as { addHost?: ProxyHost } | undefined)?.addHost;
  void load().then(() => {
    if (!disposed && initial) receiveHost(initial);
  });

  return () => {
    disposed = true;
    pendingHost = null;
    offAction?.();
    offVisibility?.();
    stopLogs();
    root.replaceChildren();
  };
}
