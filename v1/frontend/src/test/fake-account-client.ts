import { vi } from 'vitest'
import type {
  AccountClient,
  AccountCreateRequest,
  AccountGroup,
  AccountGroupCreateRequest,
  AccountGroupModifyRequest,
  AccountModifyRequest,
  AccountSnapshot,
  AccountUser,
} from '../lib/accounts/account-client'

export function accountUser(overrides: Partial<AccountUser> = {}): AccountUser {
  return {
    name: 'alice',
    uid: 1001,
    gid: 1001,
    group: 'alice',
    gecos: 'Alice Example',
    home: '/home/alice',
    shell: '/bin/bash',
    locked: false,
    system: false,
    source: 'local',
    groups: ['alice'],
    ...overrides,
  }
}

export function accountGroup(overrides: Partial<AccountGroup> = {}): AccountGroup {
  return {
    name: 'developers',
    gid: 2000,
    members: [],
    system: false,
    source: 'local',
    ...overrides,
  }
}

export function accountSnapshot(
  overrides: Partial<AccountSnapshot> = {},
): AccountSnapshot {
  const users = overrides.users ?? [accountUser()]
  const groups = overrides.groups ?? [accountGroup()]
  return {
    users,
    groups,
    control_enabled: true,
    password_enabled: false,
    allow_system: false,
    allow_privileged_groups: false,
    timestamp_ms: 1_700_000_000_000,
    total_users: users.length,
    total_groups: groups.length,
    truncated: false,
    ...overrides,
  }
}

export interface AccountCall {
  method: string
  args: unknown[]
}

/**
 * An in-memory `AccountClient`. Every call is recorded so a test can assert not
 * just the rendered result but the exact request the component produced.
 */
export class FakeAccountClient implements AccountClient {
  snapshot: AccountSnapshot
  calls: AccountCall[] = []
  error?: Error
  getError?: Error

  constructor(snapshot: AccountSnapshot = accountSnapshot()) {
    this.snapshot = snapshot
  }

  get = vi.fn(async () => {
    if (this.getError) throw this.getError
    return this.snapshot
  })

  createUser = vi.fn(async (request: AccountCreateRequest) => {
    this.record('createUser', request)
    return `created user ${request.name}`
  })

  modifyUser = vi.fn(async (name: string, request: AccountModifyRequest) => {
    this.record('modifyUser', name, request)
    return `modified user ${name}`
  })

  deleteUser = vi.fn(async (name: string, removeHome?: boolean) => {
    this.record('deleteUser', name, removeHome)
    return `deleted user ${name}`
  })

  setPassword = vi.fn(async (name: string, password: string) => {
    this.record('setPassword', name, password)
    return `password updated for ${name}`
  })

  createGroup = vi.fn(async (request: AccountGroupCreateRequest) => {
    this.record('createGroup', request)
    return `created group ${request.name}`
  })

  modifyGroup = vi.fn(async (name: string, request: AccountGroupModifyRequest) => {
    this.record('modifyGroup', name, request)
    return `modified group ${name}`
  })

  deleteGroup = vi.fn(async (name: string) => {
    this.record('deleteGroup', name)
    return `deleted group ${name}`
  })

  private record(method: string, ...args: unknown[]): void {
    if (this.error) throw this.error
    this.calls.push({ method, args })
  }
}
