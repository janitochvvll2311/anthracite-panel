/**
 * Client for `GET /api/config` and `PATCH /api/config`.
 *
 * The view is a projection of the TOML file with every secret replaced by a
 * boolean, so a form can render it without the page ever holding a digest or a
 * signing key. A patch sends only the fields the operator changed.
 */

import { responseError } from '../http'

export interface ConfigView {
  socket_path: string
  sudo: boolean
  terminal: { shell: string; max_sessions: number }
  process: { control: boolean; spawn: boolean; spawn_allow: string[] }
  network: { control: boolean }
  firewall: { control: boolean }
  volumes: { control: boolean; mount_root: string }
  accounts: {
    control: boolean
    manage_passwords: boolean
    allow_system: boolean
    allow_privileged_groups: boolean
    home_root: string
    default_shell: string
  }
  files: {
    root: string
    write: boolean
    max_entries: number
    max_preview_bytes: number
    max_upload_bytes: number
  }
  auth: {
    enabled: boolean
    username: string
    password_set: boolean
    access_token_secs: number
    refresh_token_secs: number
    cookie_secure: boolean
  }
}

/** A patch; every section is optional and absent keys keep their value. */
export type ConfigPatch = {
  [K in keyof ConfigView]?: ConfigView[K] extends object
    ? Partial<ConfigView[K]>
    : ConfigView[K]
}

export interface ConfigClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
}

export class HttpConfigClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch

  constructor(options: ConfigClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? ''
    this.fetchImpl =
      options.fetchImpl ?? ((input, init) => globalThis.fetch(input, init))
  }

  async read(): Promise<ConfigView> {
    const response = await this.fetchImpl(`${this.baseUrl}/api/config`, {
      credentials: 'same-origin',
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as ConfigView
  }

  async patch(patch: ConfigPatch): Promise<ConfigView> {
    const response = await this.fetchImpl(`${this.baseUrl}/api/config`, {
      method: 'PATCH',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(patch),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as ConfigView
  }
}
