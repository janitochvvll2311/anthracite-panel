import type { Caller, OrderEvent } from "./types";

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

/// Splits an SSE byte stream into `data:` payloads.
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

/// Follows one order's timeline until the signal aborts.
export async function watchOrder(
  context: Caller,
  order: string,
  signal: AbortSignal,
  onEvent: (event: OrderEvent) => void,
): Promise<void> {
  const response = await context.fetch(
    route(context, `/order/events?order=${encodeURIComponent(order)}`),
    { headers: { accept: "text/event-stream" }, signal },
  );
  if (!response.ok) {
    throw new Error((await response.text()) || `HTTP ${response.status}`);
  }
  if (!response.body) throw new Error("the timeline has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const feed = createSseParser((data) => {
    if (signal.aborted) return;
    try {
      onEvent(JSON.parse(data) as OrderEvent);
    } catch {
      // A malformed event is skipped; the next one still renders.
    }
  });
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
