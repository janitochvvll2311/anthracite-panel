import type { Caller, FrameContext, Snapshot } from "./types";

export async function call(
  context: Caller,
  route: string,
  init: RequestInit = {},
): Promise<any> {
  const { headers, ...rest } = init;
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}${route}`,
    { ...rest, headers: { accept: "application/json", ...(headers ?? {}) } },
  );
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

/// Follows `/watch` until the signal aborts or the engine ends the stream.
export async function watchNetwork(
  context: FrameContext,
  signal: AbortSignal,
  intervalMs: number,
  onSnapshot: (snapshot: Snapshot) => void,
): Promise<void> {
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}/watch?interval-ms=${intervalMs}`,
    { headers: { accept: "text/event-stream" }, signal },
  );
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }
  if (!response.body) throw new Error("the network watch has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const feed = createSseParser((data) => {
    if (signal.aborted) return;
    try {
      onSnapshot(JSON.parse(data) as Snapshot);
    } catch {
      // A malformed event is skipped; the next snapshot still renders.
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

export type RouteRequest = {
  family: string;
  destination: string;
  gateway: string | null;
  device: string;
  metric: number;
};

export function addRoute(context: Caller, route: RouteRequest): Promise<any> {
  return call(context, "/route", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(route),
  });
}
