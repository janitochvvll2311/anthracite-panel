import type {
  AppInstallResult,
  AppsClient,
  AppsView,
  Change,
} from 'anth-wire'

import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpAppsClient implements AppsClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  get(): Promise<AppsView> {
    return this.transport.json<AppsView>('GET', '/api/apps')
  }

  install(
    bundleBase64: string,
    acceptCapabilities: boolean,
  ): Promise<AppInstallResult> {
    return this.transport.json<AppInstallResult>('POST', '/api/apps/install', {
      bundle_base64: bundleBase64,
      accept_capabilities: acceptCapabilities,
    })
  }

  enable(id: string): Promise<Change> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/enable`)
  }

  disable(id: string): Promise<Change> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/disable`)
  }

  remove(id: string): Promise<Change> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/remove`)
  }

  approve(id: string, acceptCapabilities: boolean): Promise<Change> {
    return this.action(`/api/apps/${encodeURIComponent(id)}/approve`, {
      accept_capabilities: acceptCapabilities,
    })
  }

  private action(path: string, body: unknown = {}): Promise<Change> {
    return this.transport.json<Change>('POST', path, body)
  }
}
