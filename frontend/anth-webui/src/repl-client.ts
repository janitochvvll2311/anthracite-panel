/**
 * The three routes the REPL reaches, and nothing else.
 *
 * Separate from `repl.ts` so the executor above it can be tested without a
 * transport, and so this file is the only place that spells a route — the same
 * division `host-info-client.ts` uses.
 */

import { type HostInfo, send } from "@anth/wire";

/** The pipe a call stands in for, matching `anth_wire::PluginPipe`. */
export type PluginPipe = "cli" | "http" | "direct";

export interface PluginCall {
  id: string;
  pipe: PluginPipe;
  /** Already in the shape the chosen pipe carries. */
  payload: string;
}

/** One plugin as `GET /api/plugins` lists it. */
export interface PluginSummary {
  id: string;
  name: string;
  version: string;
  anchors: string[];
  /**
   * Where the bundle asks to be mounted, when it asks at all.
   *
   * Absent rather than empty for a plugin that declares no UI, so a caller can
   * tell "no slots" from "the field was not sent" by the same rule the daemon
   * uses. `module` is a path inside the bundle, served at `/p/<id>/<module>`.
   */
  ui?: UiSlot[];
  /** `null` when the plugin loaded; the reason when it did not. */
  disabled_reason: string | null;
}

/** One mount point a bundle declares. */
export interface UiSlot {
  slot: string;
  tag: string;
  module: string;
  label?: string;
}

export function readHostInfo(base = ""): Promise<HostInfo> {
  return send<HostInfo>({ path: `${base}/api/host-info` });
}

export async function listPlugins(base = ""): Promise<PluginSummary[]> {
  const body = await send<{ plugins: PluginSummary[] }>({
    path: `${base}/api/plugins`,
  });
  return body.plugins;
}

export function invokePlugin(
  base: string,
  request: PluginCall,
): Promise<unknown> {
  return send<unknown>({
    method: "POST",
    path: `${base}/api/plugins/${encodeURIComponent(request.id)}/call`,
    body: request,
  });
}
