// The iframe bridge: app modules never run in the panel origin.
//
// The panel fetches the module source (same-origin, hash-pinned) and sends it to
// a sandboxed frame through `postMessage`. The frame has an opaque origin: it
// cannot read the panel DOM, cookies or storage, and every app route it calls is
// proxied through a scoped RPC that only accepts the app's own
// `/api/apps/{id}/` prefix.

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
  kind: "view" | "action" | "settings";
};

export type FrameOptions = {
  source: string;
  context: FrameContext;
  close?: () => void;
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
  let cleanup = (): void => {};

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
      if (message.type === "anth:resize") {
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
        void request(target, {
          method: init.method ?? "GET",
          headers: new Headers(init.headers ?? []),
          body: init.body ?? null,
        })
          .then(async (response) => {
            const headers: Record<string, string> = {};
            response.headers.forEach((value, name) => {
              headers[name.toLowerCase()] = value;
            });
            postFetch(iframe, token, id, {
              ok: response.ok,
              status: response.status,
              statusText: response.statusText,
              headers,
              body: await response.text(),
            } as RpcResult);
          })
          .catch((error: unknown) => {
            postFetch(iframe, token, id, {
              error: error instanceof Error ? error.message : String(error),
            } as RpcResult);
          });
      }
    };

    cleanup = () => {
      window.clearTimeout(timer);
      window.removeEventListener("message", listener);
      iframe.contentWindow?.postMessage({ type: "anth:unmount", token }, "*");
      iframe.remove();
    };

    window.addEventListener("message", listener);
  });

  host.replaceChildren(iframe);
  return result;
}
