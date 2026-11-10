import type { FrameContext } from "./types";

export async function call(
  context: FrameContext,
  route: string,
  init: RequestInit = {},
): Promise<any> {
  const { headers, ...rest } = init;
  const timeout =
    !rest.signal &&
    typeof AbortSignal !== "undefined" &&
    "timeout" in AbortSignal
      ? AbortSignal.timeout(8_000)
      : undefined;
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}${route}`,
    {
      ...rest,
      ...(timeout ? { signal: timeout } : {}),
      headers: { accept: "application/json", ...(headers ?? {}) },
    },
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
