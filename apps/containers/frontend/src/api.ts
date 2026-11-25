import type { Caller, Sample } from "./types";

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

/// Reads a binary response body (logs, exec) and calls `onChunk` per frame.
export async function readBytes(
  response: Response,
  signal: AbortSignal,
  onChunk: (bytes: Uint8Array) => void,
): Promise<void> {
  if (!response.body) throw new Error("the stream has no body");
  const reader = response.body.getReader();
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
      onChunk(value);
    }
  } finally {
    signal.removeEventListener("abort", cancel);
  }
}

export async function watchLogs(
  context: Caller,
  id: string,
  signal: AbortSignal,
  onText: (text: string) => void,
): Promise<void> {
  const response = await context.fetch(
    route(
      context,
      `/container/logs?id=${encodeURIComponent(id)}&tail=400&follow=1`,
    ),
    { headers: { accept: "application/octet-stream" }, signal },
  );
  if (!response.ok) {
    throw new Error((await response.text()) || `HTTP ${response.status}`);
  }
  const decoder = new TextDecoder();
  await readBytes(response, signal, (bytes) => {
    onText(decoder.decode(bytes, { stream: true }));
  });
}

export async function watchStats(
  context: Caller,
  id: string,
  signal: AbortSignal,
  onSample: (sample: Sample) => void,
): Promise<void> {
  const response = await context.fetch(
    route(context, `/container/stats?id=${encodeURIComponent(id)}`),
    { headers: { accept: "text/event-stream" }, signal },
  );
  if (!response.ok) {
    throw new Error((await response.text()) || `HTTP ${response.status}`);
  }
  if (!response.body) throw new Error("the stats stream has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const feed = createSseParser((data) => {
    try {
      onSample(JSON.parse(data) as Sample);
    } catch {
      // A malformed sample is skipped.
    }
  });
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    feed(decoder.decode(value, { stream: true }));
  }
}

export async function watchExec(
  context: Caller,
  session: string,
  signal: AbortSignal,
  onText: (text: string) => void,
): Promise<void> {
  const response = await context.fetch(
    route(
      context,
      `/container/exec/attach?session=${encodeURIComponent(session)}`,
    ),
    { headers: { accept: "application/octet-stream" }, signal },
  );
  if (!response.ok) {
    throw new Error((await response.text()) || `HTTP ${response.status}`);
  }
  const decoder = new TextDecoder();
  await readBytes(response, signal, (bytes) => {
    onText(decoder.decode(bytes, { stream: true }));
  });
}
