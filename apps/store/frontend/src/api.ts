import type { Caller } from "./types";

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
