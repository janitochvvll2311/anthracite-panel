// The iframe bridge: app modules never run in the panel origin.
//
// The panel fetches the module source (same-origin, hash-pinned) and sends it to
// a sandboxed frame through `postMessage`. The frame has an opaque origin: it
// cannot read the panel DOM, cookies or storage, and every app route it calls is
// proxied through a scoped RPC that only accepts the app's own
// `/api/apps/{id}/` prefix.

import { isStreamingContentType } from "./stream";

export type FrameContext = {
  app: { id: string; version: string; risk: string };
  contribution: {
    id: string;
    label: string;
    icon?: string;
    order: number;
    tag: string;
    module: string;
    sha256: string;
    app: string;
  };
  panel: { slots: string[]; version: string };
  kind: "view" | "action" | "settings" | "modal";
  /// Opaque data the panel passed when opening this view.
  params?: unknown;
};

/// A request from an app module to open another of its own views.
export type OpenViewRequest = {
  tag: string;
  params?: unknown;
  /// `tab` (default) or `float`, so a view can pop straight into a panel window.
  mode?: "tab" | "float";
  /// Preferred size when the panel has to create a window.
  size?: { width?: number; height?: number };
};

/// A request from an app module to open one of its own modals.
export type OpenModalRequest = {
  tag: string;
  params?: unknown;
};

/// A live frame the panel can talk to after it mounted.
export type FrameHandle = {
  /// Delivers a transient open request without remounting the view.
  update: (params: unknown) => void;
  /// Asks the module whether it may close (an unsaved-changes guard).
  requestClose: () => Promise<boolean>;
  /// Reads the current opaque view state, e.g. before a float/dock remount.
  snapshot: () => Promise<unknown>;
  /// Tells the module whether its pane is on screen, so it can pause polling.
  setVisible: (visible: boolean) => void;
  /// Sends a local action tag and an optional payload to the module.
  action: (tag: string, payload?: unknown) => void;
};

export type FrameOptions = {
  source: string;
  context: FrameContext;
  close?: () => void;
  /// Opens another contribution of the same app at panel level.
  openView?: (request: OpenViewRequest) => boolean | void;
  /// Opens one of the app's modal contributions.
  openModal?: (request: OpenModalRequest) => boolean | void;
  /// Records opaque parameters the module wants to keep across float/dock.
  onParams?: (params: unknown) => void;
  /// Called once the module mounted, so the panel can send later requests.
  onReady?: (frame: FrameHandle) => void;
  /// A same-origin fetch, so a host can add authentication or logging.
  request?: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
  frameUrl?: string;
};

type RpcResult = {
  id: string;
  ok: boolean;
  status: number;
  statusText: string;
  headers: Record<string, string>;
  body: string;
  error?: string;
};

let sequence = 0;

const FRAME_STYLE_ID = "anth-frame-defaults";

/// Defaults live in a zero-specificity rule, so a panel's own `.app-frame`
/// rules win instead of fighting inline styles.
function ensureFrameStyles(): void {
  if (document.getElementById(FRAME_STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = FRAME_STYLE_ID;
  style.textContent =
    ":where(.app-frame){display:block;width:100%;height:160px;border:0;background:transparent;color-scheme:dark;}";
  document.head.append(style);
}

/// `crypto.randomUUID` is only available in a secure context; a panel served
/// over plain HTTP still needs an instance id.
function instanceId(): string {
  const webCrypto = globalThis.crypto;
  if (webCrypto && "randomUUID" in webCrypto) return webCrypto.randomUUID();
  sequence += 1;
  return `${Date.now().toString(36)}-${sequence.toString(36)}-${Math.random()
    .toString(36)
    .slice(2)}`;
}

function postFetch(
  iframe: HTMLIFrameElement,
  token: string,
  id: string,
  result: Omit<RpcResult, "id">,
): void {
  iframe.contentWindow?.postMessage(
    { type: "anth:fetch-result", token, id, result },
    "*",
  );
}

/// Mounts one app module in a sandboxed frame. Resolves once the module called
/// `mount`; the returned function removes the frame.
export async function mountFrame(
  host: HTMLElement,
  options: FrameOptions,
): Promise<() => void> {
  ensureFrameStyles();
  const token = instanceId();
  const allow = `/api/apps/${encodeURIComponent(options.context.app.id)}/`;
  const iframe = document.createElement("iframe");
  iframe.className = "app-frame";
  iframe.setAttribute("sandbox", "allow-scripts allow-forms allow-downloads");
  iframe.setAttribute(
    "title",
    `${options.context.contribution.label} — ${options.context.app.id}`,
  );
  iframe.src = options.frameUrl ?? "/app-frame.html";

  let settled = false;
  let ready = false;
  let filled = false;
  let cleanup = (): void => {};
  let closeSequence = 0;
  const closePending = new Map<
    string,
    { resolve: (ok: boolean) => void; timer: number }
  >();
  let snapshotSequence = 0;
  const snapshotPending = new Map<string, (value: unknown) => void>();
  /// In-flight streaming fetches, so a cancel or an unmount closes the engine
  /// response instead of leaving a producer running. The frame pulls one chunk
  /// at a time, which is what carries backpressure to the service.
  type LiveStream = {
    controller: AbortController;
    reader?: ReadableStreamDefaultReader<Uint8Array>;
    reading: boolean;
  };
  const streams = new Map<string, LiveStream>();
  /// WebSocket routes opened for this frame. The frame never opens a socket
  /// directly: it has an opaque origin and no cookie. One frame is in flight at
  /// a time and the frame acknowledges it after xterm consumes it, which is how
  /// backpressure reaches the engine.
  type LiveSocket = {
    socket: WebSocket;
    queue: ArrayBuffer[];
    inFlight: boolean;
  };
  const sockets = new Map<string, LiveSocket>();

  const flushSocket = (id: string): void => {
    const live = sockets.get(id);
    if (!live || live.inFlight || live.queue.length === 0) return;
    const data = live.queue.shift()!;
    live.inFlight = true;
    iframe.contentWindow?.postMessage(
      { type: "anth:ws-message", token, id, data },
      "*",
      [data],
    );
  };

  const update = (params: unknown): void => {
    iframe.contentWindow?.postMessage(
      { type: "anth:update-params", token, params },
      "*",
    );
  };

  const setVisible = (visible: boolean): void => {
    iframe.contentWindow?.postMessage(
      { type: "anth:visibility", token, visible },
      "*",
    );
  };

  const action = (tag: string, payload?: unknown): void => {
    iframe.contentWindow?.postMessage(
      { type: "anth:action", token, tag, payload },
      "*",
    );
  };

  const requestClose = (): Promise<boolean> =>
    new Promise((resolve) => {
      const requestId = `${token}-close-${++closeSequence}`;
      const timer = window.setTimeout(() => {
        const pending = closePending.get(requestId);
        if (!pending) return;
        closePending.delete(requestId);
        pending.resolve(false);
      }, 2000);
      closePending.set(requestId, { resolve, timer });
      iframe.contentWindow?.postMessage(
        { type: "anth:request-close", token, requestId },
        "*",
      );
    });

  const requestSnapshot = (): Promise<unknown> =>
    new Promise((resolve) => {
      const requestId = `${token}-snapshot-${++snapshotSequence}`;
      snapshotPending.set(requestId, resolve);
      iframe.contentWindow?.postMessage(
        { type: "anth:request-snapshot", token, requestId },
        "*",
      );
      window.setTimeout(() => {
        const pending = snapshotPending.get(requestId);
        if (!pending) return;
        snapshotPending.delete(requestId);
        pending(null);
      }, 2000);
    });

  const result = new Promise<() => void>((resolve, reject) => {
    const timer = window.setTimeout(() => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new Error("the app module did not mount in time"));
    }, 10_000);

    const finish = (error?: Error): void => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      if (error) {
        cleanup();
        reject(error);
      } else {
        resolve(cleanup);
      }
    };

    const listener = (event: MessageEvent): void => {
      if (event.source !== iframe.contentWindow) return;
      const message = event.data as { type?: string; token?: string } & Record<
        string,
        unknown
      >;
      if (!message || typeof message.type !== "string") return;

      if (message.type === "anth:frame-ready" && !ready) {
        ready = true;
        iframe.contentWindow?.postMessage(
          {
            type: "anth:mount",
            token,
            source: options.source,
            context: options.context,
          },
          "*",
        );
        return;
      }
      if (message.token !== token) return;

      if (message.type === "anth:mounted") {
        options.onReady?.({
          update,
          requestClose,
          snapshot: requestSnapshot,
          setVisible,
          action,
        });
        finish();
        return;
      }
      if (message.type === "anth:error") {
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
          ok =
            options.openView?.({
              tag,
              params: message.params,
              mode: message.mode === "float" ? "float" : "tab",
              size: message.size as OpenViewRequest["size"],
            }) !== false;
        }
        if (requestId) {
          iframe.contentWindow?.postMessage(
            { type: "anth:open-view-result", token, requestId, ok },
            "*",
          );
        }
        return;
      }
      if (message.type === "anth:open-modal") {
        const tag = String(message.tag ?? "");
        const requestId = String(message.requestId ?? "");
        let ok = false;
        if (tag) {
          ok =
            options.openModal?.({
              tag,
              params: message.params,
            }) !== false;
        }
        if (requestId) {
          iframe.contentWindow?.postMessage(
            { type: "anth:open-modal-result", token, requestId, ok },
            "*",
          );
        }
        return;
      }
      if (message.type === "anth:escape") {
        if (options.context.kind === "modal") options.close?.();
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
          if (pending.timer) window.clearTimeout(pending.timer);
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
        if (filled) return;
        const height = Math.max(120, Number(message.height) || 0);
        iframe.style.height = `${height}px`;
        return;
      }
      if (message.type === "anth:fetch") {
        const id = String(message.id ?? "");
        const url = String(message.url ?? "");
        const init = (message.init ?? {}) as {
          method?: string;
          headers?: [string, string][];
          body?: string | null;
        };
        let target: URL;
        try {
          target = new URL(url, location.origin);
        } catch {
          postFetch(iframe, token, id, { error: "invalid URL" } as RpcResult);
          return;
        }
        if (
          target.origin !== location.origin ||
          !target.pathname.startsWith(allow)
        ) {
          postFetch(iframe, token, id, {
            error: "this module may only call its own app routes",
          } as RpcResult);
          return;
        }
        const request = options.request ?? fetch;
        const live: LiveStream = {
          controller: new AbortController(),
          reading: false,
        };
        streams.set(id, live);
        void request(target, {
          method: init.method ?? "GET",
          headers: new Headers(init.headers ?? []),
          body: init.body ?? null,
          signal: live.controller.signal,
        })
          .then(async (response) => {
            const headers: Record<string, string> = {};
            response.headers.forEach((value, name) => {
              headers[name.toLowerCase()] = value;
            });
            const streaming =
              response.body != null &&
              isStreamingContentType(headers["content-type"]);
            if (!streaming) {
              const body = await response.text();
              streams.delete(id);
              postFetch(iframe, token, id, {
                ok: response.ok,
                status: response.status,
                statusText: response.statusText,
                headers,
                body,
              } as RpcResult);
              return;
            }
            // The head resolves the frame's `Response`. The frame then asks for
            // one chunk per pull; the reader stays paused in between.
            live.reader = response.body!.getReader();
            iframe.contentWindow?.postMessage(
              {
                type: "anth:stream-head",
                token,
                id,
                ok: response.ok,
                status: response.status,
                statusText: response.statusText,
                headers,
              },
              "*",
            );
          })
          .catch((error: unknown) => {
            streams.delete(id);
            if (live.controller.signal.aborted) return;
            postFetch(iframe, token, id, {
              error: error instanceof Error ? error.message : String(error),
            } as RpcResult);
          });
        return;
      }
      if (message.type === "anth:stream-pull") {
        const id = String(message.id ?? "");
        const live = streams.get(id);
        if (!live || !live.reader || live.reading) return;
        live.reading = true;
        void live.reader
          .read()
          .then(({ done, value }) => {
            live.reading = false;
            if (done) {
              streams.delete(id);
              iframe.contentWindow?.postMessage(
                { type: "anth:stream-end", token, id },
                "*",
              );
              return;
            }
            if (!value) {
              streams.delete(id);
              iframe.contentWindow?.postMessage(
                { type: "anth:stream-end", token, id },
                "*",
              );
              return;
            }
            const copy = new Uint8Array(value);
            iframe.contentWindow?.postMessage(
              { type: "anth:stream-chunk", token, id, data: copy.buffer },
              "*",
              [copy.buffer],
            );
          })
          .catch((error: unknown) => {
            live.reading = false;
            streams.delete(id);
            if (live.controller.signal.aborted) {
              iframe.contentWindow?.postMessage(
                { type: "anth:stream-end", token, id },
                "*",
              );
              return;
            }
            iframe.contentWindow?.postMessage(
              {
                type: "anth:stream-end",
                token,
                id,
                error: error instanceof Error ? error.message : String(error),
              },
              "*",
            );
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
        let target: URL;
        try {
          target = new URL(url, location.origin);
        } catch {
          iframe.contentWindow?.postMessage(
            {
              type: "anth:ws-open-result",
              token,
              id,
              ok: false,
              error: "invalid URL",
            },
            "*",
          );
          return;
        }
        if (
          target.origin !== location.origin ||
          !target.pathname.startsWith(allow)
        ) {
          iframe.contentWindow?.postMessage(
            {
              type: "anth:ws-open-result",
              token,
              id,
              ok: false,
              error: "this module may only open its own app routes",
            },
            "*",
          );
          return;
        }
        const scheme = target.protocol === "https:" ? "wss" : "ws";
        const socket = new WebSocket(
          `${scheme}://${target.host}${target.pathname}${target.search}`,
        );
        socket.binaryType = "arraybuffer";
        const live: LiveSocket = { socket, queue: [], inFlight: false };
        sockets.set(id, live);
        socket.onopen = () => {
          iframe.contentWindow?.postMessage(
            { type: "anth:ws-open-result", token, id, ok: true },
            "*",
          );
        };
        socket.onmessage = (event) => {
          if (event.data instanceof ArrayBuffer) {
            const entry = sockets.get(id);
            if (!entry) return;
            entry.queue.push(event.data);
            flushSocket(id);
          } else if (typeof event.data === "string") {
            iframe.contentWindow?.postMessage(
              { type: "anth:ws-message", token, id, text: event.data },
              "*",
            );
          }
        };
        socket.onclose = (event) => {
          sockets.delete(id);
          iframe.contentWindow?.postMessage(
            {
              type: "anth:ws-close",
              token,
              id,
              code: event.code,
              reason: event.reason,
            },
            "*",
          );
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
          live.socket.send(data as ArrayBuffer);
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
        if (pending.timer) window.clearTimeout(pending.timer);
        pending.resolve(true);
      }
      closePending.clear();
      for (const resolve of snapshotPending.values()) resolve(null);
      snapshotPending.clear();
      for (const live of streams.values()) live.controller.abort();
      streams.clear();
      for (const live of sockets.values()) live.socket.close();
      sockets.clear();
      iframe.contentWindow?.postMessage({ type: "anth:unmount", token }, "*");
      iframe.remove();
    };

    window.addEventListener("message", listener);
  });

  host.replaceChildren(iframe);
  return result;
}
