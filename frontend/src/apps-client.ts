import { responseError } from './http'

export interface AppAnchor {
  method: string
  path: string
}

export interface AppUiSlot {
  slot: string
  tag: string
  module: string
  label?: string
  sha256: string
}

export interface AppView {
  id: string
  alias?: string
  name: string
  version: string
  enabled: boolean
  state: 'enabled' | 'disabled' | 'error' | 'unapproved'
  error?: string
  capabilities: string[]
  api: AppAnchor[]
  ui: AppUiSlot[]
}

export interface AppsView {
  enabled: boolean
  install_from_web: boolean
  apps: AppView[]
}

export interface AppInstallResult {
  id: string
  name: string
  version: string
  capabilities: string[]
  warning?: string
}

export interface AppsClient {
  get(): Promise<AppsView>
  install(bundleBase64: string, acceptCapabilities: boolean): Promise<AppInstallResult>
  enable(id: string): Promise<string>
  disable(id: string): Promise<string>
  remove(id: string): Promise<string>
  approve(id: string, acceptCapabilities: boolean): Promise<string>
}

export class HttpAppsClient implements AppsClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<AppsView> {
    const response = await this.fetchImpl('/api/apps')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as AppsView
  }

  async install(
    bundleBase64: string,
    acceptCapabilities: boolean,
  ): Promise<AppInstallResult> {
    const response = await this.fetchImpl('/api/apps/install', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        bundle_base64: bundleBase64,
        accept_capabilities: acceptCapabilities,
      }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as AppInstallResult
  }

  async enable(id: string): Promise<string> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/enable`)
  }

  async disable(id: string): Promise<string> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/disable`)
  }

  async remove(id: string): Promise<string> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/remove`)
  }

  async approve(id: string, acceptCapabilities: boolean): Promise<string> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/approve`, {
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
