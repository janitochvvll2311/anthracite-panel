import { responseError } from './http'
import type { Settings, SessionsView, SettingsClient } from 'anth-wire'

export class HttpSettingsClient implements SettingsClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the other clients: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<Settings> {
    const response = await this.fetchImpl('/api/settings')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as Settings
  }

  async update(key: string, value: unknown): Promise<{ restart_required: boolean }> {
    const response = await this.fetchImpl('/api/settings', {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ key, value }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as { restart_required: boolean }
  }

  async restart(): Promise<void> {
    const response = await this.fetchImpl('/api/restart', { method: 'POST' })
    if (!response.ok) throw await responseError(response)
  }

  async reboot(delaySeconds: number): Promise<void> {
    const response = await this.fetchImpl('/api/reboot', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ delay_s: delaySeconds }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async sessions(): Promise<SessionsView> {
    const response = await this.fetchImpl('/api/auth/sessions')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as SessionsView
  }

  async revokeOthers(): Promise<number> {
    const response = await this.fetchImpl('/api/auth/sessions/revoke', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ others: true }),
    })
    if (!response.ok) throw await responseError(response)
    const body = (await response.json()) as { revoked?: unknown }
    return typeof body.revoked === 'number' ? body.revoked : 0
  }
}
