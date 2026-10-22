/**
 * Plugin UI anchors: fetch the catalogue and load the declared modules.
 *
 * The shell treats a plugin as data until the moment it mounts. A plugin names
 * a slot the shell already knows, a custom element tag and a module under its
 * own `web/` directory; the shell loads that module with a dynamic `import()`
 * and appends the element to the slot. No plugin ships a component the shell
 * imports statically, so a plugin that fails to load cannot break the bundle.
 *
 * The module runs on the panel's origin, with the operator's session. That is
 * a deliberate, documented limit of this design rather than an oversight: the
 * wasm sandbox protects the daemon from a plugin's server half, not the
 * browser from its client half. See `docs/design/plugins.md`.
 */

export interface PluginUiAnchor {
  slot: string
  tag: string
  module: string
}

export interface PluginSummary {
  id: string
  name: string
  version: string
  enabled: boolean
  ui: PluginUiAnchor[]
}

export interface PluginsCatalogue {
  enabled: boolean
  install_from_web: boolean
  plugins: PluginSummary[]
  problems: string[]
}

export interface PluginClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
}

export interface PluginClient {
  list(): Promise<PluginsCatalogue>
  /** URL an anchor's module is served from. */
  moduleUrl(pluginId: string, module: string): string
}

export class HttpPluginClient implements PluginClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch

  constructor(options: PluginClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? ''
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
  }

  async list(): Promise<PluginsCatalogue> {
    const response = await this.fetchImpl(`${this.baseUrl}/api/plugins`)
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`)
    return (await response.json()) as PluginsCatalogue
  }

  moduleUrl(pluginId: string, module: string): string {
    // `module` is `web/...` as the manifest declares it, and the asset route is
    // `/api/plugins/<id>/web/<path>`, so the prefix is stripped rather than
    // duplicated. Encoding keeps an id with unusual bytes from splitting the
    // path.
    const relative = module.startsWith('web/') ? module.slice(4) : module
    const parts = relative.split('/').map(encodeURIComponent)
    return `${this.baseUrl}/api/plugins/${encodeURIComponent(pluginId)}/web/${parts.join('/')}`
  }
}

/**
 * Anchors for one slot, from enabled plugins only.
 *
 * A disabled plugin's modules are not listed: the asset route refuses them, so
 * offering the anchor would only produce a failed import in the console.
 */
export function anchorsBySlot(
  catalogue: PluginsCatalogue | undefined,
  slot: string,
): { plugin: string; anchor: PluginUiAnchor }[] {
  if (!catalogue?.enabled) return []
  const found: { plugin: string; anchor: PluginUiAnchor }[] = []
  for (const plugin of catalogue.plugins) {
    if (!plugin.enabled) continue
    for (const anchor of plugin.ui) {
      if (anchor.slot === slot) found.push({ plugin: plugin.id, anchor })
    }
  }
  return found
}

/**
 * Loads every module for a slot and returns the elements to mount.
 *
 * An import or construction failure is reported and skipped. One broken plugin
 * must not empty the dashboard, and the failure belongs in the console with the
 * plugin id attached so the operator can find it.
 */
export async function loadSlot(
  client: PluginClient,
  catalogue: PluginsCatalogue | undefined,
  slot: string,
): Promise<HTMLElement[]> {
  const elements: HTMLElement[] = []
  for (const { plugin, anchor } of anchorsBySlot(catalogue, slot)) {
    try {
      await import(/* @vite-ignore */ client.moduleUrl(plugin, anchor.module))
      elements.push(document.createElement(anchor.tag))
    } catch (error) {
      console.error(
        `[plugin] ${plugin} failed to load ${anchor.tag} for ${slot}:`,
        error,
      )
    }
  }
  return elements
}
