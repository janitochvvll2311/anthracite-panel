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

import { responseError } from '../http'

export interface PluginUiAnchor {
  slot: string
  tag: string
  module: string
  /** Tab label for panel slots; falls back to the plugin name. */
  label?: string
}

export interface PluginSummary {
  id: string
  name: string
  version: string
  enabled: boolean
  capabilities: string[]
  api: { method: string; path: string }[]
  ui: PluginUiAnchor[]
}

export interface PluginsCatalogue {
  enabled: boolean
  install_from_web: boolean
  plugins: PluginSummary[]
  problems: string[]
}

/** Result of installing one bundle from the browser. */
export interface PluginInstallResult {
  id: string
  version: string
  name: string
  /** Present when the bundle was accepted without a verified signature. */
  warning?: string | null
}

export interface PluginClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
}

export interface PluginClient {
  list(): Promise<PluginsCatalogue>
  /** Installs one zip bundle. Refused unless `install_from_web` is on. */
  install(file: File): Promise<PluginInstallResult>
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
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as PluginsCatalogue
  }

  async install(file: File): Promise<PluginInstallResult> {
    const response = await this.fetchImpl(`${this.baseUrl}/api/plugins/install`, {
      method: 'POST',
      headers: { 'content-type': 'application/zip' },
      body: file,
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as PluginInstallResult
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
 * Loads one anchor's module and returns the element it declares.
 *
 * The module is imported once per realm. `data-slot` lets a module that serves
 * both a dashboard widget and a full panel pick the right presentation without
 * a second file.
 */
export async function loadAnchor(
  client: Pick<PluginClient, 'moduleUrl'>,
  plugin: string,
  anchor: PluginUiAnchor,
): Promise<HTMLElement> {
  await import(/* @vite-ignore */ client.moduleUrl(plugin, anchor.module))
  const element = document.createElement(anchor.tag)
  element.dataset.slot = anchor.slot
  return element
}

/**
 * Loads every module for a slot and returns the elements to mount.
 *
 * An import or construction failure is reported and skipped. One broken plugin
 * must not empty the dashboard, and the failure belongs in the console with the
 * plugin id attached so the operator can find it.
 */
export async function loadSlot(
  client: Pick<PluginClient, 'moduleUrl'>,
  catalogue: PluginsCatalogue | undefined,
  slot: string,
): Promise<HTMLElement[]> {
  const elements: HTMLElement[] = []
  for (const { plugin, anchor } of anchorsBySlot(catalogue, slot)) {
    try {
      elements.push(await loadAnchor(client, plugin, anchor))
    } catch (error) {
      console.error(
        `[plugin] ${plugin} failed to load ${anchor.tag} for ${slot}:`,
        error,
      )
    }
  }
  return elements
}
