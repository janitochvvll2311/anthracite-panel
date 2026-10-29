import type {
  AccountClient,
  AccountCreate,
  AccountDelete,
  AccountGroupCreate,
  AccountGroupDelete,
  AccountSetPassword,
  AccountSnapshot,
  Change,
} from 'anth-wire'

import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpAccountClient implements AccountClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  get(): Promise<AccountSnapshot> {
    return this.transport.json<AccountSnapshot>('GET', '/api/accounts')
  }

  createUser(request: AccountCreate): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/accounts/users', request)
  }

  deleteUser(request: AccountDelete): Promise<Change> {
    return this.transport.json<Change>(
      'POST',
      '/api/accounts/users/delete',
      request,
    )
  }

  setPassword(request: AccountSetPassword): Promise<Change> {
    return this.transport.json<Change>(
      'POST',
      '/api/accounts/users/password',
      request,
    )
  }

  createGroup(request: AccountGroupCreate): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/accounts/groups', request)
  }

  deleteGroup(request: AccountGroupDelete): Promise<Change> {
    return this.transport.json<Change>(
      'POST',
      '/api/accounts/groups/delete',
      request,
    )
  }
}
