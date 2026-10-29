import { responseError } from './http'
import type { AppsView, AppInstallResult, AppsClient } from 'anth-wire'

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
