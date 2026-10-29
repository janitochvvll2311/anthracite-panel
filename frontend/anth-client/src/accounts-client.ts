import { responseError } from './http'
import type { AccountSnapshot, AccountCreate, AccountDelete, AccountGroupCreate, AccountClient } from 'anth-wire'

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
