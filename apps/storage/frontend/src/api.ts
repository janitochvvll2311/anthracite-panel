import type { Caller } from "./types";

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

export type MountRequest = {
  device: string;
  mountpoint: string;
  filesystem: string;
};

export function mountDevice(context: Caller, request: MountRequest): Promise<any> {
  return call(context, "/mount", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(request),
  });
}
