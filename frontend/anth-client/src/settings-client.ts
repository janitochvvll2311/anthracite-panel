import type {
  Rebooting,
  Restarting,
  SessionsView,
  Settings,
  SettingsClient,
} from 'anth-wire'

import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpSettingsClient implements SettingsClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  get(): Promise<Settings> {
    return this.transport.json<Settings>('GET', '/api/settings')
  }

  update(
    key: string,
    value: unknown,
  ): Promise<{ restart_required: boolean }> {
    return this.transport.json<{ restart_required: boolean }>(
      'PATCH',
      '/api/settings',
      { key, value },
    )
  }

  restart(): Promise<Restarting> {
    return this.transport.json<Restarting>('POST', '/api/restart')
  }

  reboot(delaySeconds: number): Promise<Rebooting> {
    return this.transport.json<Rebooting>('POST', '/api/reboot', {
      delay_s: delaySeconds,
    })
  }

  sessions(): Promise<SessionsView> {
    return this.transport.json<SessionsView>('GET', '/api/auth/sessions')
  }

  async revokeOthers(): Promise<number> {
    const body = await this.transport.json<{ revoked?: unknown }>(
      'POST',
      '/api/auth/sessions/revoke',
      { others: true },
    )
    return typeof body.revoked === 'number' ? body.revoked : 0
  }
}
