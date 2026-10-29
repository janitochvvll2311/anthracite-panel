import type { AuthClient, AuthState } from 'anth-wire'

import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpAuthClient implements AuthClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  state(): Promise<AuthState> {
    return this.transport.json<AuthState>('GET', '/api/auth/state')
  }

  login(username: string, password: string): Promise<void> {
    return this.transport.call('POST', '/api/auth/login', {
      username,
      password,
    })
  }

  register(password: string): Promise<void> {
    return this.transport.call('POST', '/api/auth/register', { password })
  }

  logout(): Promise<void> {
    return this.transport.call('POST', '/api/auth/logout')
  }

  password(current: string, next: string): Promise<void> {
    return this.transport.call('POST', '/api/auth/password', {
      current_password: current,
      new_password: next,
    })
  }
}
