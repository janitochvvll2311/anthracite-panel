// src/stream.ts
function isStreamingContentType(contentType) {
  if (!contentType)
    return false;
  const type = contentType.split(";", 1)[0].trim().toLowerCase();
  return type === "application/octet-stream" || type === "text/event-stream";
}

// src/testids.ts
var TESTID = {
  topbar: "topbar",
  brand: "brand",
  health: "health",
  actions: "actions",
  actionButton: "action-button",
  managerButton: "manager-button",
  settingsButton: "settings-button",
  navToggle: "nav-toggle",
  drawerBackdrop: "drawer-backdrop",
  drawerClose: "drawer-close",
  sidebar: "sidebar",
  views: "views",
  viewItem: "view-item",
  viewsPlaceholder: "views-placeholder",
  warnings: "warnings",
  warning: "warning",
  developer: "developer",
  developerToggle: "developer-toggle",
  viewportSelect: "viewport-select",
  viewportEffective: "viewport-effective",
  panelVersion: "panel-version",
  workspace: "workspace",
  tabs: "tabs",
  tab: "tab",
  tabSelect: "tab-select",
  tabFloat: "tab-float",
  tabClose: "tab-close",
  panes: "panes",
  emptyState: "empty-state",
  pane: "pane",
  paneHeader: "pane-header",
  paneTitle: "pane-title",
  paneApp: "pane-app",
  paneTools: "pane-tools",
  paneFloat: "pane-float",
  paneClose: "pane-close",
  paneBody: "pane-body",
  paneSubtabs: "pane-subtabs",
  subTab: "sub-tab",
  fabs: "fabs",
  fab: "fab",
  fabButton: "fab-button",
  fabMenu: "fab-menu",
  fabItem: "fab-item",
  windows: "windows",
  window: "window",
  windowHeader: "window-header",
  windowTitle: "window-title",
  windowApp: "window-app",
  windowTools: "window-tools",
  windowTool: "window-tool",
  windowBody: "window-body",
  stack: "stack",
  stackItem: "stack-item",
  stackOpen: "stack-open",
  stackClose: "stack-close",
  modal: "modal",
  modalTitle: "modal-title",
  modalApp: "modal-app",
  modalBody: "modal-body",
  modalClose: "modal-close",
  settings: "settings",
  settingsBody: "settings-body",
  settingsClose: "settings-close",
  settingsSection: "settings-section",
  settingsEmpty: "settings-empty",
  manager: "manager",
  managerBody: "manager-body",
  managerClose: "manager-close",
  managerStatus: "manager-status",
  managerApps: "manager-apps",
  managerApp: "manager-app",
  managerCompose: "manager-compose",
  managerComposeItem: "manager-compose-item",
  managerAudit: "manager-audit",
  errorCard: "error-card",
  loading: "loading",
  appFrame: "app-frame",
  slotRoot: "slot-root",
  loginForm: "login-form",
  loginPassword: "login-password",
  loginSubmit: "login-submit",
  loginStatus: "login-status",
  setupForm: "setup-form",
  setupCurrent: "setup-current",
  setupNew: "setup-new",
  setupSubmit: "setup-submit",
  setupStatus: "setup-status"
};
function testId(node, id) {
  node.dataset.testid = id;
  return node;
}

// src/iframe.ts
var sequence = 0;
var FRAME_STYLE_ID = "anth-frame-defaults";
function ensureFrameStyles() {
  if (document.getElementById(FRAME_STYLE_ID))
    return;
  const style = document.createElement("style");
  style.id = FRAME_STYLE_ID;
  style.textContent = ":where(.app-frame){display:block;width:100%;height:160px;border:0;background:transparent;color-scheme:dark;}";
  document.head.append(style);
}
function instanceId() {
  const webCrypto = globalThis.crypto;
  if (webCrypto && "randomUUID" in webCrypto)
    return webCrypto.randomUUID();
  sequence += 1;
  return `${Date.now().toString(36)}-${sequence.toString(36)}-${Math.random().toString(36).slice(2)}`;
}
function postFetch(iframe, token, id, result) {
  iframe.contentWindow?.postMessage({ type: "anth:fetch-result", token, id, result }, "*");
}
async function mountFrame(host, options) {
  ensureFrameStyles();
  const token = instanceId();
  const allow = `/api/apps/${encodeURIComponent(options.context.app.id)}/`;
  const iframe = document.createElement("iframe");
  iframe.className = "app-frame";
  iframe.dataset.testid = TESTID.appFrame;
  iframe.dataset.app = options.context.app.id;
  iframe.dataset.slot = options.context.contribution.id;
  iframe.dataset.tag = options.context.contribution.tag;
  iframe.dataset.kind = options.context.kind;
  iframe.setAttribute("sandbox", "allow-scripts allow-forms allow-downloads");
  iframe.setAttribute("title", `${options.context.contribution.label} — ${options.context.app.id}`);
  iframe.src = options.frameUrl ?? "/app-frame.html";
  let settled = false;
  let ready = false;
  let filled = false;
  let cleanup = () => {};
  let closeSequence = 0;
  const closePending = new Map;
  let snapshotSequence = 0;
  const snapshotPending = new Map;
  const streams = new Map;
  const sockets = new Map;
  const flushSocket = (id) => {
    const live = sockets.get(id);
    if (!live || live.inFlight || live.queue.length === 0)
      return;
    const data = live.queue.shift();
    live.inFlight = true;
    iframe.contentWindow?.postMessage({ type: "anth:ws-message", token, id, data }, "*", [data]);
  };
  const update = (params) => {
    iframe.contentWindow?.postMessage({ type: "anth:update-params", token, params }, "*");
  };
  const setVisible = (visible) => {
    iframe.contentWindow?.postMessage({ type: "anth:visibility", token, visible }, "*");
  };
  const setViewport = (viewport) => {
    iframe.contentWindow?.postMessage({ type: "anth:viewport", token, viewport }, "*");
  };
  const focus = () => {
    iframe.contentWindow?.postMessage({ type: "anth:focus", token }, "*");
  };
  const action = (tag, payload) => {
    iframe.contentWindow?.postMessage({ type: "anth:action", token, tag, payload }, "*");
  };
  const requestClose = () => new Promise((resolve) => {
    const requestId = `${token}-close-${++closeSequence}`;
    const timer = window.setTimeout(() => {
      const pending = closePending.get(requestId);
      if (!pending)
        return;
      closePending.delete(requestId);
      pending.resolve(false);
    }, 2000);
    closePending.set(requestId, { resolve, timer });
    iframe.contentWindow?.postMessage({ type: "anth:request-close", token, requestId }, "*");
  });
  const requestSnapshot = () => new Promise((resolve) => {
    const requestId = `${token}-snapshot-${++snapshotSequence}`;
    snapshotPending.set(requestId, resolve);
    iframe.contentWindow?.postMessage({ type: "anth:request-snapshot", token, requestId }, "*");
    window.setTimeout(() => {
      const pending = snapshotPending.get(requestId);
      if (!pending)
        return;
      snapshotPending.delete(requestId);
      pending(null);
    }, 2000);
  });
  const result = new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => {
      if (settled)
        return;
      settled = true;
      cleanup();
      reject(new Error("the app module did not mount in time"));
    }, 1e4);
    const finish = (error) => {
      if (settled)
        return;
      settled = true;
      window.clearTimeout(timer);
      if (error) {
        cleanup();
        reject(error);
      } else {
        resolve(cleanup);
      }
    };
    const listener = (event) => {
      if (event.source !== iframe.contentWindow)
        return;
      const message = event.data;
      if (!message || typeof message.type !== "string")
        return;
      if (message.type === "anth:frame-ready" && !ready) {
        ready = true;
        iframe.contentWindow?.postMessage({
          type: "anth:mount",
          token,
          source: options.source,
          context: options.context
        }, "*");
        return;
      }
      if (message.token !== token)
        return;
      if (message.type === "anth:mounted") {
        iframe.dataset.state = "mounted";
        options.onReady?.({
          update,
          requestClose,
          snapshot: requestSnapshot,
          setVisible,
          setViewport,
          action,
          focus
        });
        finish();
        return;
      }
      if (message.type === "anth:error") {
        iframe.dataset.state = "error";
        finish(new Error(String(message.message ?? "the app module failed")));
        return;
      }
      if (message.type === "anth:close") {
        options.close?.();
        return;
      }
      if (message.type === "anth:open-view") {
        const tag = String(message.tag ?? "");
        const requestId = String(message.requestId ?? "");
        let ok = false;
        if (tag) {
          ok = options.openView?.({
            tag,
            params: message.params,
            mode: message.mode === "float" ? "float" : "tab",
            size: message.size
          }) !== false;
        }
        if (requestId) {
          iframe.contentWindow?.postMessage({ type: "anth:open-view-result", token, requestId, ok }, "*");
        }
        return;
      }
      if (message.type === "anth:open-modal") {
        const tag = String(message.tag ?? "");
        const requestId = String(message.requestId ?? "");
        let ok = false;
        if (tag) {
          ok = options.openModal?.({
            tag,
            params: message.params
          }) !== false;
        }
        if (requestId) {
          iframe.contentWindow?.postMessage({ type: "anth:open-modal-result", token, requestId, ok }, "*");
        }
        return;
      }
      if (message.type === "anth:escape") {
        if (options.context.kind === "modal")
          options.close?.();
        return;
      }
      if (message.type === "anth:params") {
        options.onParams?.(message.params);
        return;
      }
      if (message.type === "anth:close-pending") {
        const requestId = String(message.requestId ?? "");
        const pending = closePending.get(requestId);
        if (pending) {
          window.clearTimeout(pending.timer);
          pending.timer = 0;
        }
        return;
      }
      if (message.type === "anth:close-result") {
        const requestId = String(message.requestId ?? "");
        const pending = closePending.get(requestId);
        if (pending) {
          if (pending.timer)
            window.clearTimeout(pending.timer);
          closePending.delete(requestId);
          pending.resolve(message.ok !== false);
        }
        return;
      }
      if (message.type === "anth:snapshot-result") {
        const requestId = String(message.requestId ?? "");
        const pending = snapshotPending.get(requestId);
        if (pending) {
          snapshotPending.delete(requestId);
          pending(message.value ?? null);
        }
        return;
      }
      if (message.type === "anth:fill") {
        filled = true;
        host.classList.add("app-fill");
        iframe.classList.add("app-frame-fill");
        iframe.style.height = "100%";
        iframe.style.minHeight = "0";
        return;
      }
      if (message.type === "anth:resize") {
        if (filled)
          return;
        const height = Math.max(120, Number(message.height) || 0);
        iframe.style.height = `${height}px`;
        return;
      }
      if (message.type === "anth:fetch") {
        const id = String(message.id ?? "");
        const url = String(message.url ?? "");
        const init = message.init ?? {};
        let target;
        try {
          target = new URL(url, location.origin);
        } catch {
          postFetch(iframe, token, id, { error: "invalid URL" });
          return;
        }
        if (target.origin !== location.origin || !target.pathname.startsWith(allow)) {
          postFetch(iframe, token, id, {
            error: "this module may only call its own app routes"
          });
          return;
        }
        const request = options.request ?? fetch;
        const live = {
          controller: new AbortController,
          reading: false
        };
        streams.set(id, live);
        request(target, {
          method: init.method ?? "GET",
          headers: new Headers(init.headers ?? []),
          body: init.body ?? null,
          signal: live.controller.signal
        }).then(async (response) => {
          const headers = {};
          response.headers.forEach((value, name) => {
            headers[name.toLowerCase()] = value;
          });
          const streaming = response.body != null && isStreamingContentType(headers["content-type"]);
          if (!streaming) {
            const body = await response.text();
            streams.delete(id);
            postFetch(iframe, token, id, {
              ok: response.ok,
              status: response.status,
              statusText: response.statusText,
              headers,
              body
            });
            return;
          }
          live.reader = response.body.getReader();
          iframe.contentWindow?.postMessage({
            type: "anth:stream-head",
            token,
            id,
            ok: response.ok,
            status: response.status,
            statusText: response.statusText,
            headers
          }, "*");
        }).catch((error) => {
          streams.delete(id);
          if (live.controller.signal.aborted)
            return;
          postFetch(iframe, token, id, {
            error: error instanceof Error ? error.message : String(error)
          });
        });
        return;
      }
      if (message.type === "anth:stream-pull") {
        const id = String(message.id ?? "");
        const live = streams.get(id);
        if (!live || !live.reader || live.reading)
          return;
        live.reading = true;
        live.reader.read().then(({ done, value }) => {
          live.reading = false;
          if (done) {
            streams.delete(id);
            iframe.contentWindow?.postMessage({ type: "anth:stream-end", token, id }, "*");
            return;
          }
          if (!value) {
            streams.delete(id);
            iframe.contentWindow?.postMessage({ type: "anth:stream-end", token, id }, "*");
            return;
          }
          const copy = new Uint8Array(value);
          iframe.contentWindow?.postMessage({ type: "anth:stream-chunk", token, id, data: copy.buffer }, "*", [copy.buffer]);
        }).catch((error) => {
          live.reading = false;
          streams.delete(id);
          if (live.controller.signal.aborted) {
            iframe.contentWindow?.postMessage({ type: "anth:stream-end", token, id }, "*");
            return;
          }
          iframe.contentWindow?.postMessage({
            type: "anth:stream-end",
            token,
            id,
            error: error instanceof Error ? error.message : String(error)
          }, "*");
        });
        return;
      }
      if (message.type === "anth:stream-cancel") {
        const id = String(message.id ?? "");
        const live = streams.get(id);
        if (live) {
          streams.delete(id);
          live.controller.abort();
        }
        return;
      }
      if (message.type === "anth:ws-open") {
        const id = String(message.id ?? "");
        const url = String(message.url ?? "");
        let target;
        try {
          target = new URL(url, location.origin);
        } catch {
          iframe.contentWindow?.postMessage({
            type: "anth:ws-open-result",
            token,
            id,
            ok: false,
            error: "invalid URL"
          }, "*");
          return;
        }
        if (target.origin !== location.origin || !target.pathname.startsWith(allow)) {
          iframe.contentWindow?.postMessage({
            type: "anth:ws-open-result",
            token,
            id,
            ok: false,
            error: "this module may only open its own app routes"
          }, "*");
          return;
        }
        const scheme = target.protocol === "https:" ? "wss" : "ws";
        const socket = new WebSocket(`${scheme}://${target.host}${target.pathname}${target.search}`);
        socket.binaryType = "arraybuffer";
        const live = { socket, queue: [], inFlight: false };
        sockets.set(id, live);
        socket.onopen = () => {
          iframe.contentWindow?.postMessage({ type: "anth:ws-open-result", token, id, ok: true }, "*");
        };
        socket.onmessage = (event) => {
          if (event.data instanceof ArrayBuffer) {
            const entry = sockets.get(id);
            if (!entry)
              return;
            entry.queue.push(event.data);
            flushSocket(id);
          } else if (typeof event.data === "string") {
            iframe.contentWindow?.postMessage({ type: "anth:ws-message", token, id, text: event.data }, "*");
          }
        };
        socket.onclose = (event) => {
          sockets.delete(id);
          iframe.contentWindow?.postMessage({
            type: "anth:ws-close",
            token,
            id,
            code: event.code,
            reason: event.reason
          }, "*");
        };
        return;
      }
      if (message.type === "anth:ws-ack") {
        const live = sockets.get(String(message.id ?? ""));
        if (live) {
          live.inFlight = false;
          flushSocket(String(message.id ?? ""));
        }
        return;
      }
      if (message.type === "anth:ws-send") {
        const id = String(message.id ?? "");
        const live = sockets.get(id);
        const data = message.data;
        if (live && live.socket.readyState === WebSocket.OPEN && data) {
          live.socket.send(data);
        }
        return;
      }
      if (message.type === "anth:ws-close") {
        const id = String(message.id ?? "");
        const live = sockets.get(id);
        if (live) {
          sockets.delete(id);
          live.socket.close();
        }
        return;
      }
    };
    cleanup = () => {
      window.clearTimeout(timer);
      window.removeEventListener("message", listener);
      for (const pending of closePending.values()) {
        if (pending.timer)
          window.clearTimeout(pending.timer);
        pending.resolve(true);
      }
      closePending.clear();
      for (const resolve of snapshotPending.values())
        resolve(null);
      snapshotPending.clear();
      for (const live of streams.values())
        live.controller.abort();
      streams.clear();
      for (const live of sockets.values())
        live.socket.close();
      sockets.clear();
      iframe.contentWindow?.postMessage({ type: "anth:unmount", token }, "*");
      iframe.remove();
    };
    window.addEventListener("message", listener);
  });
  host.replaceChildren(iframe);
  return result;
}
// src/dom.ts
function byId(id) {
  const element = document.getElementById(id);
  if (!element)
    throw new Error(`missing #${id}`);
  return element;
}
function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className)
    node.className = className;
  if (text !== undefined)
    node.textContent = text;
  return node;
}
function button(className, text, title) {
  const node = element("button", className, text);
  node.type = "button";
  if (title)
    node.title = title;
  return node;
}
function safeDispose(dispose) {
  try {
    dispose();
  } catch (error) {
    console.error("disposing a module failed", error);
  }
}
function errorCard(error, {
  className = "error-card",
  title = "This module did not mount",
  detailTag = "p"
} = {}) {
  const card = element("div", className);
  card.dataset.testid = TESTID.errorCard;
  card.append(element("strong", "", title), element(detailTag, "", error instanceof Error ? error.message : String(error)));
  return card;
}
function frameContext(contribution, kind, panel, params) {
  return {
    app: {
      id: contribution.app.id,
      version: contribution.app.version,
      risk: contribution.app.risk
    },
    contribution: { ...contribution.slot, app: contribution.app.id },
    panel: {
      slots: [...panel.slots],
      version: panel.version,
      ...panel.viewport === undefined ? {} : { viewport: panel.viewport }
    },
    kind,
    ...params === undefined ? {} : { params }
  };
}

// src/index.ts
var VIEW = "panel.view";
var ACTION = "panel.action";
var SETTINGS = "panel.settings";
var FAB = "panel.fab";
var MODAL = "panel.modal";
var REFRESH = "refresh";
var PORTABLE = [VIEW, ACTION, SETTINGS, FAB, MODAL];
function contributionKey(contribution) {
  return `${contribution.app.id}#${contribution.slot.id}#${contribution.slot.tag}`;
}
function encodeModule(module) {
  return module.split("/").map(encodeURIComponent).join("/");
}
function assetUrl(contribution) {
  const app = encodeURIComponent(contribution.app.id);
  const module = encodeModule(contribution.slot.module);
  return `/api/apps/${app}/assets/${module}?v=${contribution.slot.sha256.slice(0, 16)}`;
}
function compare(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}
function ordered(apps, slot) {
  return apps.flatMap((app) => app.slots.filter((candidate) => candidate.id === slot).map((candidate) => ({ app, slot: candidate }))).sort((left, right) => left.slot.order - right.slot.order || compare(left.app.id, right.app.id) || compare(left.slot.tag, right.slot.tag));
}
function ignored(apps, implemented) {
  return apps.flatMap((app) => app.slots.filter((slot) => !implemented.includes(slot.id)).map((slot) => ({ app, slot })));
}
export {
  ACTION,
  FAB,
  MODAL,
  PORTABLE,
  REFRESH,
  SETTINGS,
  TESTID,
  VIEW,
  assetUrl,
  button,
  byId,
  contributionKey,
  element,
  encodeModule,
  errorCard,
  frameContext,
  ignored,
  isStreamingContentType,
  mountFrame,
  ordered,
  safeDispose,
  testId
};
