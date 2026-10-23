/**
 * The browser half of the plugin contract.
 *
 * The daemon runs a plugin's wasm module; the panel mounts its web module. This
 * package is the second half, mirroring `anth-plugin` on the Rust side. It is
 * three pieces, and a plugin picks the ones it needs:
 *
 * - **`PluginWidget`** — an interface, not a base class. It states what the host
 *   needs: `load` and `view`, with optional `update`, `loading`, `failure` and
 *   `body`. A plugin's element extends whatever it likes — `HTMLElement`,
 *   `LitElement` — and implements this.
 * - **`mountWidget`** — the optional helper that does the plumbing: shadow root,
 *   load states, retry button, and keeping an element instead of replacing
 *   markup. A plugin already comfortable in its framework can skip it.
 * - **`PluginEndpoints`** — a configurable client for the plugin's own routes:
 *   `get`, `send`, `delete`, with the origin, headers and fetch injectable.
 *
 * ```ts
 * import { PluginEndpoints, mountWidget, registerWidget, type PluginWidget } from "@anth/plugin";
 *
 * interface Greeting { message: string }
 *
 * export class AnthExample extends HTMLElement implements PluginWidget<Greeting> {
 *   readonly endpoints = new PluginEndpoints({ pluginId: "hello" }); // id or alias
 *   readonly handle = mountWidget(this, this);
 *
 *   load(): Promise<Greeting> {
 *     return this.endpoints.get("/greeting");
 *   }
 *
 *   view(data: Greeting): string {
 *     return `<p>${escapeHtml(data.message)}</p>`;
 *   }
 * }
 *
 * registerWidget("anth-example", AnthExample);
 * ```
 *
 * The host's rules a plugin must still follow, which no library can enforce for
 * it: only relative endpoint paths (the client rejects an absolute one), only the
 * capabilities the manifest granted, and no assumption that a module is imported
 * once.
 */

export {
  ANCHOR_PREFIX,
  PluginEndpoints,
  anchorError,
  type PluginAnchor,
  type PluginCatalogue,
  type PluginEndpointsOptions,
  type PluginSummary,
  type RequestOptions,
} from "./endpoints";

export {
  escapeHtml,
  type MarkupWidget,
  type PluginWidget,
  type WidgetContent,
  type WidgetState,
} from "./contract";

export { mountWidget, type WidgetHandle } from "./mount";

export { currentSlot, registerWidget } from "./register";
