import { responseError } from './http'

export type AccountSource = 'local' | 'nss'

export interface AccountUser {
  name: string
  uid: number
  gid: number
  group: string
  gecos: string
  home: string
  shell: string
  locked?: boolean
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
  allow_system: boolean
  allow_privileged_groups: boolean
  timestamp_ms: number
  warning?: string
}

export interface AccountCreate {
  name: string
  uid?: number
  gid?: number
  gecos?: string
  home?: string
  shell?: string
  system: boolean
}

export interface AccountDelete {
  name: string
  remove_home: boolean
}

export interface AccountGroupCreate {
  name: string
  gid?: number
  system: boolean
}

export interface AccountClient {
  get(): Promise<AccountSnapshot>
  createUser(request: AccountCreate): Promise<string>
  deleteUser(request: AccountDelete): Promise<string>
  setPassword(name: string, password: string): Promise<string>
  createGroup(request: AccountGroupCreate): Promise<string>
  deleteGroup(name: string): Promise<string>
}

export class HttpAccountClient implements AccountClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<AccountSnapshot> {
    const response = await this.fetchImpl('/api/accounts')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as AccountSnapshot
  }

  async createUser(request: AccountCreate): Promise<string> {
    return this.action('/api/accounts/users', request)
  }

  async deleteUser(request: AccountDelete): Promise<string> {
    return this.action('/api/accounts/users/delete', request)
  }

  async setPassword(name: string, password: string): Promise<string> {
    return this.action('/api/accounts/users/password', { name, password })
  }

  async createGroup(request: AccountGroupCreate): Promise<string> {
    return this.action('/api/accounts/groups', request)
  }

  async deleteGroup(name: string): Promise<string> {
    return this.action('/api/accounts/groups/delete', { name })
  }

  private async action(path: string, body: unknown): Promise<string> {
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
