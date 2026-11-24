import { call } from "./api";
import { styles } from "./styles";
import type {
  ModalContext,
  ProxyConfig,
  ProxyHost,
  ProxyListener,
  ProxyRoute,
  ProxyUpstream,
} from "./types";

const extra = `
  .pxm { display: flex; flex-direction: column; gap: 12px; }
  .pxm label { display: flex; flex-direction: column; gap: 4px; color: #98a2b6; font-size: 12px; }
  .pxm input[type="text"], .pxm input[type="number"], .pxm select {
    width: 100%; padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 13px;
  }
  .pxm input:disabled { color: #7a8497; }
  .pxm-row { display: flex; gap: 14px; flex-wrap: wrap; }
  .pxm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .pxm button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .pxm button.primary { border-color: #33507a; background: #16233a; color: #dbe6ff; }
  .pxm-status { margin: 0; font-size: 12px; color: #8f98ab; }
  .pxm-status.bad { color: #ffb3ba; }
  @media (max-width: 720px) {
    .pxm-actions { flex-direction: column-reverse; }
    .pxm-actions button { width: 100%; }
  }
`;

function frame(root: HTMLElement): {
  style: HTMLStyleElement;
  box: HTMLElement;
} {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);
  const box = document.createElement("div");
  box.className = "pxm";
  root.append(box);
  return { style, box };
}

function status(box: HTMLElement): HTMLElement {
  return box.querySelector("[data-status]") as HTMLElement;
}

function setStatus(box: HTMLElement, message: string, bad = false): void {
  const node = status(box);
  node.textContent = message;
  node.className = bad ? "pxm-status bad" : "pxm-status";
}

function text(box: HTMLElement, name: string): string {
  return (
    box.querySelector<HTMLInputElement>(`[name="${name}"]`)?.value.trim() ?? ""
  );
}

function number(box: HTMLElement, name: string, fallback: number): number {
  const value = Number(text(box, name));
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

function checked(box: HTMLElement, name: string): boolean {
  return box.querySelector<HTMLInputElement>(`[name="${name}"]`)?.checked ?? false;
}

function options(
  values: Array<{ value: string; label: string }>,
  selected: string,
): string {
  return values
    .map(
      (entry) =>
        `<option value="${escape(entry.value)}"${
          entry.value === selected ? " selected" : ""
        }>${escape(entry.label)}</option>`,
    )
    .join("");
}

function escape(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

async function loadConfig(context: ModalContext): Promise<ProxyConfig> {
  return (await call(context, "/config")) as ProxyConfig;
}

function actions(): string {
  return `<div class="pxm-actions">
    <button type="button" data-cancel data-testid="proxy-modal-cancel">Cancel</button>
    <button type="button" class="primary" data-submit data-testid="proxy-modal-submit">Save</button>
  </div>
  <p class="pxm-status" role="status" data-status data-testid="proxy-modal-status"></p>`;
}

function wire(box: HTMLElement, context: ModalContext, commit: () => Promise<void>): void {
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  box.querySelector("[data-cancel]")?.addEventListener("click", () => context.close());
  submit.addEventListener("click", () => void commit());
  box.addEventListener("keydown", (event) => {
    if ((event as KeyboardEvent).key === "Enter") {
      event.preventDefault();
      void commit();
    }
  });
}

async function deliver(
  box: HTMLElement,
  context: ModalContext,
  params: Record<string, unknown>,
): Promise<void> {
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  submit.disabled = true;
  const opened = await context.openView?.({ tag: "anth-proxy", params });
  if (!opened) {
    submit.disabled = false;
    setStatus(box, "The panel did not hand the draft back to the view.", true);
    return;
  }
  context.close();
}

function hostForm(box: HTMLElement, context: ModalContext): () => void {
  const host = (context.params as { host?: ProxyHost } | undefined)?.host;
  let disposed = false;
  box.innerHTML = `
    <label>Id<input type="text" name="id" data-testid="proxy-host-id" value="${escape(
      host?.id ?? "",
    )}" ${host ? "disabled" : ""} placeholder="site" /></label>
    <label>Names<input type="text" name="names" data-testid="proxy-host-names" value="${escape(
      host?.names.join(", ") ?? "",
    )}" placeholder="example.com, www.example.com" /></label>
    <label>Certificate<select name="certificate" data-testid="proxy-host-certificate"><option value="">(none)</option></select></label>
    <div class="pxm-row">
      <label class="px-check"><input type="checkbox" name="acme" data-testid="proxy-host-acme" ${
        host?.acme ? "checked" : ""
      } /> serve ACME HTTP-01</label>
      <label class="px-check"><input type="checkbox" name="redirect" data-testid="proxy-host-redirect" ${
        host?.["redirect-https"] ? "checked" : ""
      } /> redirect HTTP to HTTPS</label>
      <label class="px-check"><input type="checkbox" name="hsts" data-testid="proxy-host-hsts" ${
        host?.hsts ? "checked" : ""
      } /> HSTS</label>
    </div>
    ${actions()}`;
  const certificate = box.querySelector(
    'select[name="certificate"]',
  ) as HTMLSelectElement;
  void (async () => {
    try {
      const ids = (await call(context, "/certificates")) as string[];
      if (disposed) return;
      for (const id of Array.isArray(ids) ? ids : []) {
        const option = document.createElement("option");
        option.value = id;
        option.textContent = id;
        certificate.append(option);
      }
      certificate.value = host?.certificate ?? "";
    } catch (error) {
      if (!disposed) setStatus(box, String(error), true);
    }
  })();
  wire(box, context, async () => {
    const id = text(box, "id");
    const names = text(box, "names")
      .split(",")
      .map((name) => name.trim().toLowerCase())
      .filter(Boolean);
    if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
      setStatus(box, "The id is lowercase letters, digits and dashes.", true);
      return;
    }
    if (names.length === 0) {
      setStatus(box, "At least one host name is required.", true);
      return;
    }
    const next: ProxyHost = {
      id,
      names,
      certificate: certificate.value || null,
      acme: checked(box, "acme"),
      "redirect-https": checked(box, "redirect"),
      hsts: checked(box, "hsts"),
      routes: host?.routes ?? [],
    };
    await deliver(
      box,
      context,
      host ? { upsertHost: next } : { addHost: next },
    );
  });
  (box.querySelector('[name="id"]') as HTMLInputElement | null)?.focus();
  return () => {
    disposed = true;
  };
}

function upstreamForm(box: HTMLElement, context: ModalContext): () => void {
  const upstream = (
    context.params as { upstream?: ProxyUpstream } | undefined
  )?.upstream;
  box.innerHTML = `
    <label>Id<input type="text" name="id" data-testid="proxy-upstream-id" value="${escape(
      upstream?.id ?? "",
    )}" ${upstream ? "disabled" : ""} placeholder="app" /></label>
    <label>URL<input type="text" name="url" data-testid="proxy-upstream-url" value="${escape(
      upstream?.url ?? "",
    )}" placeholder="http://127.0.0.1:3000" /></label>
    <div class="pxm-row">
      <label>Connect ms<input type="number" min="1" max="300000" name="connect" data-testid="proxy-upstream-connect" value="${
        upstream?.["connect-timeout-ms"] ?? 2000
      }" /></label>
      <label>Read ms<input type="number" min="1" max="300000" name="read" data-testid="proxy-upstream-read" value="${
        upstream?.["read-timeout-ms"] ?? 5000
      }" /></label>
    </div>
    <label class="px-check"><input type="checkbox" name="verify" data-testid="proxy-upstream-verify" ${
      upstream ? (upstream["tls-verify"] ? "checked" : "") : "checked"
    } /> verify the upstream certificate</label>
    ${actions()}`;
  wire(box, context, async () => {
    const id = text(box, "id");
    const url = text(box, "url");
    if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
      setStatus(box, "The id is lowercase letters, digits and dashes.", true);
      return;
    }
    if (!/^https?:\/\//.test(url)) {
      setStatus(box, "The URL starts with http:// or https://.", true);
      return;
    }
    const next: ProxyUpstream = {
      id,
      url,
      "connect-timeout-ms": number(box, "connect", 2000),
      "read-timeout-ms": number(box, "read", 5000),
      "tls-verify": checked(box, "verify"),
    };
    await deliver(
      box,
      context,
      upstream ? { upsertUpstream: next } : { addUpstream: next },
    );
  });
  (box.querySelector('[name="id"]') as HTMLInputElement | null)?.focus();
  return () => {};
}

function routeForm(box: HTMLElement, context: ModalContext): () => void {
  const params = context.params as
    | { hostId?: string; route?: ProxyRoute; upstreams?: string[] }
    | undefined;
  const route = params?.route;
  const hostId = params?.hostId ?? "";
  let disposed = false;
  box.innerHTML = `
    <label>Match<input type="text" name="match" data-testid="proxy-route-match" value="${escape(
      route?.match ?? "",
    )}" placeholder="= /health or /" /></label>
    <label>Upstream<select name="upstream" data-testid="proxy-route-upstream"><option value="">(none)</option></select></label>
    <label>Redirect<input type="text" name="redirect" data-testid="proxy-route-redirect" value="${escape(
      route?.redirect ?? "",
    )}" placeholder="optional https://… or /path" /></label>
    <div class="pxm-row">
      <label class="px-check"><input type="checkbox" name="strip" data-testid="proxy-route-strip" ${
        route?.["strip-prefix"] ? "checked" : ""
      } /> strip the prefix</label>
      <label class="px-check"><input type="checkbox" name="websocket" data-testid="proxy-route-websocket" ${
        route?.websocket ? "checked" : ""
      } /> WebSocket</label>
      <label>Body limit<input type="number" min="1" name="limit" data-testid="proxy-route-limit" value="${
        route?.["body-limit"] ?? ""
      }" placeholder="default" /></label>
    </div>
    ${actions()}`;
  const upstream = box.querySelector(
    'select[name="upstream"]',
  ) as HTMLSelectElement;
  void (async () => {
    try {
      const ids =
        params?.upstreams ??
        (await loadConfig(context)).upstreams.map((entry) => entry.id);
      if (disposed) return;
      for (const id of ids) {
        const option = document.createElement("option");
        option.value = id;
        option.textContent = id;
        upstream.append(option);
      }
      upstream.value = route?.upstream ?? "";
    } catch (error) {
      if (!disposed) setStatus(box, String(error), true);
    }
  })();
  wire(box, context, async () => {
    const match = text(box, "match");
    const redirect = text(box, "redirect");
    const selected = upstream.value;
    if (!match.startsWith("/") && !match.startsWith("= /")) {
      setStatus(box, "The match starts with `/` or `= /`.", true);
      return;
    }
    if (!redirect && !selected) {
      setStatus(box, "Choose an upstream or write a redirect.", true);
      return;
    }
    const limit = text(box, "limit");
    const next: ProxyRoute = {
      id: route?.id ?? "",
      match,
      upstream: redirect ? null : selected,
      "strip-prefix": checked(box, "strip"),
      websocket: checked(box, "websocket"),
      redirect: redirect || null,
      headers: route?.headers ?? [],
      "body-limit": limit ? Number(limit) : null,
    };
    await deliver(box, context, { upsertRoute: { hostId, route: next } });
  });
  (box.querySelector('[name="match"]') as HTMLInputElement | null)?.focus();
  return () => {
    disposed = true;
  };
}

function listenerForm(box: HTMLElement, context: ModalContext): () => void {
  const params = context.params as
    | { listener?: ProxyListener; hosts?: Array<{ id: string; names: string[] }> }
    | undefined;
  const listener = params?.listener;
  let disposed = false;
  box.innerHTML = `
    <label>Id<input type="text" value="${escape(listener?.id ?? "")}" disabled /></label>
    <label>Bind<input type="text" value="${escape(listener?.bind ?? "")}" disabled /></label>
    <label>Default host<select name="default-host" data-testid="proxy-listener-default"><option value="">(none)</option></select></label>
    <label class="px-check"><input type="checkbox" name="forwarded" data-testid="proxy-listener-forwarded" ${
      listener?.forwarded ? "checked" : ""
    } /> trust and set X-Forwarded-*</label>
    ${actions()}`;
  const select = box.querySelector(
    'select[name="default-host"]',
  ) as HTMLSelectElement;
  void (async () => {
    try {
      const hosts =
        params?.hosts ??
        (await loadConfig(context)).hosts.map((host) => ({
          id: host.id,
          names: host.names,
        }));
      if (disposed) return;
      for (const host of hosts) {
        const option = document.createElement("option");
        option.value = host.id;
        option.textContent = `${host.id} (${host.names.join(", ")})`;
        select.append(option);
      }
      select.value = listener?.["default-host"] ?? "";
    } catch (error) {
      if (!disposed) setStatus(box, String(error), true);
    }
  })();
  wire(box, context, async () => {
    if (!listener) {
      setStatus(box, "The listener is missing.", true);
      return;
    }
    const next: ProxyListener = {
      ...listener,
      "default-host": select.value || null,
      forwarded: checked(box, "forwarded"),
    };
    await deliver(box, context, { upsertListener: next });
  });
  return () => {
    disposed = true;
  };
}

export function mount(root: HTMLElement, context: ModalContext): () => void {
  const { box } = frame(root);
  let disposeInner: () => void;
  switch (context.contribution.tag) {
    case "proxy-upstream-modal":
      disposeInner = upstreamForm(box, context);
      break;
    case "proxy-route-modal":
      disposeInner = routeForm(box, context);
      break;
    case "proxy-listener-modal":
      disposeInner = listenerForm(box, context);
      break;
    default:
      disposeInner = hostForm(box, context);
  }
  return () => {
    disposeInner();
    root.replaceChildren();
  };
}
