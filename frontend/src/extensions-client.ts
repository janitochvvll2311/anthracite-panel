import { responseError } from './http'

export interface ExtensionAnchor {
  method: string
  path: string
}

export interface ExtensionUiSlot {
  slot: string
  tag: string
  module: string
  label?: string
  sha256: string
}

export interface ExtensionView {
  id: string
  alias?: string
  name: string
  version: string
  enabled: boolean
  state: 'enabled' | 'disabled' | 'error' | 'unapproved'
  error?: string
  capabilities: string[]
  api: ExtensionAnchor[]
  ui: ExtensionUiSlot[]
}

export interface ExtensionsView {
  enabled: boolean
  install_from_web: boolean
  extensions: ExtensionView[]
}

export interface ExtensionInstallResult {
  id: string
  name: string
  version: string
  capabilities: string[]
  warning?: string
}

export interface ExtensionsClient {
  get(): Promise<ExtensionsView>
  install(bundleBase64: string, acceptCapabilities: boolean): Promise<ExtensionInstallResult>
  enable(id: string): Promise<string>
  disable(id: string): Promise<string>
  remove(id: string): Promise<string>
  approve(id: string, acceptCapabilities: boolean): Promise<string>
}

export class HttpExtensionsClient implements ExtensionsClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<ExtensionsView> {
    const response = await this.fetchImpl('/api/extensions')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as ExtensionsView
  }

  async install(
    bundleBase64: string,
    acceptCapabilities: boolean,
  ): Promise<ExtensionInstallResult> {
    const response = await this.fetchImpl('/api/extensions/install', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        bundle_base64: bundleBase64,
        accept_capabilities: acceptCapabilities,
      }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as ExtensionInstallResult
  }

  async enable(id: string): Promise<string> {
    return this.action(`/api/extensions/${encodeURIComponent(id)}/enable`)
  }

  async disable(id: string): Promise<string> {
    return this.action(`/api/extensions/${encodeURIComponent(id)}/disable`)
  }

  async remove(id: string): Promise<string> {
    return this.action(`/api/extensions/${encodeURIComponent(id)}/remove`)
  }

  async approve(id: string, acceptCapabilities: boolean): Promise<string> {
    return this.action(`/api/extensions/${encodeURIComponent(id)}/approve`, {
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
