import type {
  AccessEvent,
  Caller,
  ProxyConfig,
} from "./types";

/// The app's own route prefix. The panel frame refuses any other app.
export function route(context: Caller, path: string): string {
  return `/api/apps/${encodeURIComponent(context.app.id)}${path}`;
}

export async function call(
  context: Caller,
  path: string,
  init: RequestInit = {},
): Promise<any> {
  const { headers, ...rest } = init;
  const response = await context.fetch(route(context, path), {
    ...rest,
    headers: { accept: "application/json", ...(headers ?? {}) },
  });
  const body = await response.text();
  let value: any = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message =
      value && typeof value === "object" && value.error
        ? `${value.error.code}: ${value.error.message}`
        : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}

/// `apply` has two parameters, so the route body is keyed by name (design
/// §7.7): never a bare document.
export function applyBody(
  config: ProxyConfig,
  expectedVersion: number,
): { config: ProxyConfig; "expected-version": number } {
  return { config, "expected-version": expectedVersion };
}

/// Splits an SSE byte stream into `data:` payloads. A chunk can end anywhere,
/// so the parser keeps the partial frame until the blank line arrives.
export function createSseParser(
  onEvent: (data: string) => void,
): (chunk: string) => void {
  let buffer = "";
  return (chunk: string) => {
    buffer += chunk.replace(/\r/g, "");
    for (;;) {
      const end = buffer.indexOf("\n\n");
      if (end < 0) break;
      const block = buffer.slice(0, end);
      buffer = buffer.slice(end + 2);
      const data = block
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).replace(/^ /, ""))
        .join("\n");
      if (data) onEvent(data);
    }
  };
}

/// Follows `/logs` until the signal aborts or the engine ends the stream.
export async function watchLogs(
  context: Caller,
  signal: AbortSignal,
  onEvent: (event: AccessEvent) => void,
): Promise<void> {
  const response = await context.fetch(route(context, "/logs"), {
    headers: { accept: "text/event-stream" },
    signal,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }
  if (!response.body) throw new Error("the access log has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const feed = createSseParser((data) => {
    if (signal.aborted) return;
    try {
      onEvent(JSON.parse(data) as AccessEvent);
    } catch {
      // A malformed event is skipped; the next one still renders.
    }
  });
  // The frame bridge ignores the fetch signal, so cancel the reader itself;
  // its `cancel` hook is what tells the engine to stop the service stream.
  const cancel = () => void reader.cancel();
  if (signal.aborted) {
    void reader.cancel();
    return;
  }
  signal.addEventListener("abort", cancel, { once: true });
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      feed(decoder.decode(value, { stream: true }));
    }
    feed(decoder.decode());
  } finally {
    signal.removeEventListener("abort", cancel);
  }
}
