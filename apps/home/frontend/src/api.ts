import type { FrameContext, HostInfo } from "./types";

export async function fetchHostInfo(
  context: FrameContext,
): Promise<HostInfo> {
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}/info`,
    { headers: { accept: "application/json" } },
  );
  const text = await response.text();
  if (!response.ok) {
    throw new Error(text || `HTTP ${response.status}`);
  }
  return JSON.parse(text) as HostInfo;
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
export async function watchHostInfo(
  context: FrameContext,
  signal: AbortSignal,
  intervalMs: number,
  onInfo: (info: HostInfo) => void,
): Promise<void> {
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}/watch?interval-ms=${intervalMs}`,
    { headers: { accept: "text/event-stream" }, signal },
  );
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }
  if (!response.body) throw new Error("the host watch has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const feed = createSseParser((data) => {
    if (signal.aborted) return;
    try {
      onInfo(JSON.parse(data) as HostInfo);
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
