// A second panel, written against the SDK only. It implements `panel.view` and
// nothing else: the acid test is that `hello` appears here without this project
// importing the official panel or the app.
//
// The panel talks HTTP to an existing panel/engine. That transport is a detail;
// the contract, ordering, asset URLs and the sandboxed iframe come from
// @anthracite/panel.

import { loopbackTarget } from "./src/target";

const root = import.meta.dir;
const port = Number(process.env.PORT ?? 8091);
let target: URL;
try {
  target = loopbackTarget(
    process.env.ANTH_PANEL ?? "http://127.0.0.1:8090",
    process.env.ANTH_PANEL_ALLOW_REMOTE === "1",
  );
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
const credentials = process.env.ANTH_PANEL_AUTH ?? "operator:anthracite-dev";
const authorization = `Basic ${btoa(credentials)}`;
const upstreamOperator = process.env.ANTH_PANEL_OPERATOR ?? "operator";
const upstreamPassword = process.env.ANTH_PANEL_PASSWORD;
if (!upstreamPassword) {
  console.error(
    "ANTH_PANEL_PASSWORD is required (read it from the target's state/credentials.txt)",
  );
  process.exit(1);
}

async function login(): Promise<string> {
  const response = await fetch(new URL("/api/engine/session", target), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      operator: upstreamOperator,
      password: upstreamPassword,
    }),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`login answered ${response.status}: ${text}`);
  }
  const header = response.headers.get("set-cookie");
  if (!header) throw new Error("the target did not set a session cookie");
  return header.split(";")[0]!;
}

let sessionCookie = await login();
const appFrameUrl = new URL(
  import.meta.resolve("@anthracite/panel/app-frame.html"),
);

const SHELL_CSP =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
   connect-src 'self'; img-src 'self' data:; frame-src 'self'; object-src 'none'; \
   base-uri 'none'; frame-ancestors 'none'";
const FRAME_CSP =
  "default-src 'none'; script-src 'unsafe-inline' blob:; style-src 'unsafe-inline'; \
   img-src data:; connect-src 'none'; base-uri 'none'; frame-ancestors 'self'";

function text(body: string, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(body, {
    status,
    headers: { "content-type": "text/plain; charset=utf-8", ...headers },
  });
}

function json(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json" },
  });
}

async function file(
  path: string | URL,
  contentType: string,
  csp?: string,
): Promise<Response> {
  const body = Bun.file(path);
  if (!(await body.exists())) return text("not found", 404);
  return new Response(body, {
    headers: {
      "content-type": contentType,
      "cache-control": "no-cache",
      ...(csp ? { "content-security-policy": csp } : {}),
    },
  });
}

async function proxy(request: Request): Promise<Response> {
  const url = new URL(request.url);
  const upstream = new URL(url.pathname + url.search, target);
  if (upstream.origin !== target.origin) {
    return text("the request escaped the configured transport", 400);
  }
  const headers = new Headers(request.headers);
  headers.set("cookie", sessionCookie);
  headers.delete("host");
  headers.delete("origin");
  headers.delete("content-length");
  const body =
    request.method === "GET" || request.method === "HEAD"
      ? undefined
      : await request.arrayBuffer();
  const send = () =>
    fetch(upstream, {
      method: request.method,
      headers,
      redirect: "manual",
      body,
    });
  let response: Response;
  try {
    response = await send();
    if (response.status === 401) {
      sessionCookie = await login();
      headers.set("cookie", sessionCookie);
      response = await send();
    }
  } catch (error) {
    return text(`the panel transport is unavailable: ${error}`, 502);
  }
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers: response.headers,
  });
}

const server = Bun.serve({
  hostname: "0.0.0.0",
  port,
  async fetch(request) {
    const url = new URL(request.url);

    if (url.pathname === "/health") return json({ status: "ok" });
    if (url.pathname === "/app-frame.html") {
      return file(appFrameUrl, "text/html", FRAME_CSP);
    }

    if (request.headers.get("authorization") !== authorization) {
      return text("authentication required", 401, {
        "www-authenticate": 'Basic realm="Anthracite minimal panel", charset="UTF-8"',
      });
    }

    if (url.pathname.startsWith("/api/")) return proxy(request);
    if (url.pathname === "/" || url.pathname === "/index.html") {
      return file(`${root}/dist/index.html`, "text/html", SHELL_CSP);
    }
    if (url.pathname === "/main.js") {
      return file(`${root}/dist/main.js`, "text/javascript");
    }
    if (url.pathname === "/style.css") {
      return file(`${root}/dist/style.css`, "text/css");
    }
    if (url.pathname === "/favicon.ico") return new Response(null, { status: 204 });
    return text("not found", 404);
  },
});

console.log(
  `minimal panel listening on http://${server.hostname}:${server.port} (api ${target.href})`,
);
