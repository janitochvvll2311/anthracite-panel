/**
 * The browser half of the plugin contract.
 *
 * The daemon runs a plugin's wasm module; the panel mounts its web module. This
 * package is the second half, mirroring `anth-plugin` on the Rust side: a plugin
 * author writes behaviour, and the anchor plumbing, the load states and the
 * custom-element registration come from here.
 *
 * ```ts
 * import { PluginWidget, registerWidget } from "@anth/plugin";
 *
 * interface Greeting { message: string }
 *
 * class AnthExample extends PluginWidget<Greeting> {
 *   protected static override pluginId = "org.example.example";
 *
 *   protected load(): Promise<Greeting> {
 *     return this.client.get("/greeting");
 *   }
 *
 *   protected view(data: Greeting): string {
 *     return `<p>${escapeHtml(data.message)}</p>`;
 *   }
 * }
 *
 * registerWidget("anth-example", AnthExample);
 * ```
 *
 * The host's rules a plugin must still follow, which no library can enforce for
 * it: only relative anchor paths (the client rejects an absolute one), only the
 * capabilities the manifest granted, and no assumption that a module is imported
 * once.
 */

export {
  ANCHOR_PREFIX,
  AnchorClient,
  anchorError,
  type AnchorClientOptions,
  type AnchorErrorBody,
  type PluginAnchor,
  type PluginCatalogue,
  type PluginSummary,
} from "./anchors";

export {
  PluginWidget,
  escapeHtml,
  type WidgetOptions,
  type WidgetState,
} from "./widget";

export { currentSlot, registerWidget } from "./register";
