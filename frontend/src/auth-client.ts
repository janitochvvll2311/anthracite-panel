import { responseError } from './http'

export interface AuthState {
  required: boolean
  registered: boolean
  authenticated: boolean
  bootstrap: 'web' | 'closed'
  username: string
}

export interface AuthClient {
  state(): Promise<AuthState>
  login(username: string, password: string): Promise<void>
  register(password: string): Promise<void>
  logout(): Promise<void>
}

export class HttpAuthClient implements AuthClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Browsers reject `obj.fetch(...)` with "Illegal invocation": fetch must
    // run with `window` as `this`, never with the client instance.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async state(): Promise<AuthState> {
    const response = await this.fetchImpl('/api/auth/state')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as AuthState
  }

  async login(username: string, password: string): Promise<void> {
    const response = await this.fetchImpl('/api/auth/login', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ username, password }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async register(password: string): Promise<void> {
    const response = await this.fetchImpl('/api/auth/register', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ password }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async logout(): Promise<void> {
    const response = await this.fetchImpl('/api/auth/logout', { method: 'POST' })
    if (!response.ok) throw await responseError(response)
  }
}
