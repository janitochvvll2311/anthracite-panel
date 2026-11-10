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
