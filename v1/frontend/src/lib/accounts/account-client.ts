/**
 * HTTP client for the account endpoints.
 *
 * The component layer never talks to fetch directly, so a different backend can
 * implement the same interface. Refresh is manual: account changes are operator
 * actions, and polling a snapshot that an operator just changed would race the
 * mutation for no benefit.
 */

import { responseError } from '../http'
import type { HttpClientOptions } from '../http'

/** Where an entry came from, and therefore whether the panel may change it. */
export type AccountSource = 'local' | 'nss'

export interface AccountUser {
  name: string
  uid: number
  gid: number
  /** Primary group name, or the numeric id when it does not resolve. */
  group: string
  gecos: string
  home: string
  shell: string
  /**
   * `null` means unknown, never unlocked: the daemon could not read the shadow
   * entry. Rendering it as `unlocked` would be a dangerous guess.
   */
  locked: boolean | null
  system: boolean
  source: AccountSource
  groups: string[]
}

export interface AccountGroup {
  name: string
  gid: number
  members: string[]
  system: boolean
  source: AccountSource
}

export interface AccountSnapshot {
  users: AccountUser[]
  groups: AccountGroup[]
  control_enabled: boolean
  password_enabled: boolean
  /** Whether the daemon's policy allows touching system accounts and groups. */
  allow_system: boolean
  /** Whether the daemon's policy allows touching root-equivalent groups. */
  allow_privileged_groups: boolean
  timestamp_ms: number
  total_users: number
  total_groups: number
  truncated: boolean
  warning?: string
}

export interface AccountCreateRequest {
  name: string
  uid?: number
  gid?: number
  gecos?: string
  home?: string
  shell?: string
  system?: boolean
}

export interface AccountModifyRequest {
  gecos?: string
  shell?: string
  home?: string
  primary_gid?: number
  locked?: boolean
}

export interface AccountGroupCreateRequest {
  name: string
  gid?: number
  system?: boolean
}

export interface AccountGroupModifyRequest {
  gid?: number
  /** Replaces the member list; it does not append. */
  members?: string[]
}

export interface AccountClient {
  get: () => Promise<AccountSnapshot>
  createUser: (request: AccountCreateRequest) => Promise<string>
  modifyUser: (name: string, request: AccountModifyRequest) => Promise<string>
  deleteUser: (name: string, removeHome?: boolean) => Promise<string>
  setPassword: (name: string, password: string) => Promise<string>
  createGroup: (request: AccountGroupCreateRequest) => Promise<string>
  modifyGroup: (name: string, request: AccountGroupModifyRequest) => Promise<string>
  deleteGroup: (name: string) => Promise<string>
}

export class HttpAccountClient implements AccountClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch

  constructor(options: HttpClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api/accounts'
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
  }

  async get(): Promise<AccountSnapshot> {
    const response = await this.fetchImpl(this.baseUrl, {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as AccountSnapshot
  }

  createUser(request: AccountCreateRequest): Promise<string> {
    return this.send('/users', request, 'created')
  }

  modifyUser(name: string, request: AccountModifyRequest): Promise<string> {
    return this.send(`/users/${encodeURIComponent(name)}`, request, 'modified')
  }

  deleteUser(name: string, removeHome = false): Promise<string> {
    return this.send(
      `/users/${encodeURIComponent(name)}/delete`,
      { remove_home: removeHome },
      'deleted',
    )
  }

  setPassword(name: string, password: string): Promise<string> {
    return this.send(
      `/users/${encodeURIComponent(name)}/password`,
      { password },
      'password updated',
    )
  }

  createGroup(request: AccountGroupCreateRequest): Promise<string> {
    return this.send('/groups', request, 'created')
  }

  modifyGroup(name: string, request: AccountGroupModifyRequest): Promise<string> {
    return this.send(`/groups/${encodeURIComponent(name)}`, request, 'modified')
  }

  deleteGroup(name: string): Promise<string> {
    return this.send(`/groups/${encodeURIComponent(name)}/delete`, {}, 'deleted')
  }

  private async send(path: string, body: unknown, fallback: string): Promise<string> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await responseError(response)
    const payload = (await response.json()) as { message?: string }
    return payload.message ?? fallback
  }
}
