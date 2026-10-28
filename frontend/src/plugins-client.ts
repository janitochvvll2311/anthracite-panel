import { responseError } from './http'

export interface PluginAnchor {
  method: string
  path: string
}

export interface PluginUiSlot {
  slot: string
  tag: string
  module: string
  label?: string
  sha256: string
}

export interface PluginView {
  id: string
  alias?: string
  name: string
  version: string
  enabled: boolean
  state: 'enabled' | 'disabled' | 'error' | 'unapproved'
  error?: string
  capabilities: string[]
  api: PluginAnchor[]
  ui: PluginUiSlot[]
}

export interface PluginsView {
  enabled: boolean
  install_from_web: boolean
  plugins: PluginView[]
}

export interface PluginInstallResult {
  id: string
  name: string
  version: string
  capabilities: string[]
  warning?: string
}

export interface PluginsClient {
  get(): Promise<PluginsView>
  install(bundleBase64: string, acceptCapabilities: boolean): Promise<PluginInstallResult>
  enable(id: string): Promise<string>
  disable(id: string): Promise<string>
  remove(id: string): Promise<string>
  approve(id: string, acceptCapabilities: boolean): Promise<string>
}

export class HttpPluginsClient implements PluginsClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<PluginsView> {
    const response = await this.fetchImpl('/api/plugins')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as PluginsView
  }

  async install(
    bundleBase64: string,
    acceptCapabilities: boolean,
  ): Promise<PluginInstallResult> {
    const response = await this.fetchImpl('/api/plugins/install', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        bundle_base64: bundleBase64,
        accept_capabilities: acceptCapabilities,
      }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as PluginInstallResult
  }

  async enable(id: string): Promise<string> {
    return this.action(`/api/plugins/${encodeURIComponent(id)}/enable`)
  }

  async disable(id: string): Promise<string> {
    return this.action(`/api/plugins/${encodeURIComponent(id)}/disable`)
  }

  async remove(id: string): Promise<string> {
    return this.action(`/api/plugins/${encodeURIComponent(id)}/remove`)
  }

  async approve(id: string, acceptCapabilities: boolean): Promise<string> {
    return this.action(`/api/plugins/${encodeURIComponent(id)}/approve`, {
      accept_capabilities: acceptCapabilities,
    })
  }

  private async action(path: string, body: unknown = {}): Promise<string> {
    const response = await this.fetchImpl(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await responseError(response)
    const result = (await response.json()) as { message?: string }
    return result.message ?? ''
  }
}
