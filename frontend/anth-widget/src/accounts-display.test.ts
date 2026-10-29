import { expect, test } from 'vitest'

import {
  canDeleteGroup,
  canDeleteUser,
  canSetPassword,
  filterGroups,
  filterUsers,
  isPrivilegedGroup,
  writableUser,
} from './lib/accounts/display'
import type { AccountGroup, AccountSnapshot, AccountUser } from 'anth-wire'

function user(overrides: Partial<AccountUser> = {}): AccountUser {
  return {
    name: 'alice',
    uid: 1001,
    gid: 1001,
    group: 'alice',
    gecos: 'Alice',
    home: '/home/alice',
    shell: '/bin/sh',
    locked: false,
    system: false,
    source: 'local',
    groups: ['wheel'],
    ...overrides,
  }
}

function group(overrides: Partial<AccountGroup> = {}): AccountGroup {
  return {
    name: 'wheel',
    gid: 10,
    members: ['alice'],
    system: true,
    source: 'local',
    ...overrides,
  }
}

function snapshot(overrides: Partial<AccountSnapshot> = {}): AccountSnapshot {
  return {
    users: [user()],
    groups: [group()],
    control_enabled: true,
    password_enabled: true,
    allow_system: false,
    allow_privileged_groups: false,
    timestamp_ms: 1,
    ...overrides,
  }
}

test('users and groups filter by every visible field', () => {
  const users = [
    user(),
    user({ name: 'bob', uid: 1002, gid: 1002, group: 'bob', gecos: 'Bob', groups: [] }),
  ]
  expect(filterUsers(users, '')).toHaveLength(2)
  expect(filterUsers(users, 'BOB').map((row) => row.name)).toEqual(['bob'])
  expect(filterUsers(users, '1001').map((row) => row.name)).toEqual(['alice'])
  expect(filterUsers(users, 'wheel').map((row) => row.name)).toEqual(['alice'])
  expect(filterUsers(users, '/bin/sh')).toHaveLength(2)
  expect(filterUsers(users, 'nobody')).toHaveLength(0)

  const groups = [group(), group({ name: 'docker', gid: 999, members: ['bob'] })]
  expect(filterGroups(groups, 'docker').map((row) => row.name)).toEqual(['docker'])
  expect(filterGroups(groups, 'bob').map((row) => row.name)).toEqual(['docker'])
  expect(filterGroups(groups, '10').map((row) => row.name)).toEqual(['wheel'])
})

test('the privileged group list mirrors the daemon', () => {
  for (const name of ['sudo', 'wheel', 'admin', 'root', 'docker', 'lxd']) {
    expect(isPrivilegedGroup(name)).toBe(true)
  }
  expect(isPrivilegedGroup('users')).toBe(false)
})

test('writable users exclude root, nss and system entries without the opt-in', () => {
  const policy = snapshot()
  expect(writableUser(user(), policy)).toBe(true)
  expect(writableUser(user({ uid: 0 }), policy)).toBe(false)
  expect(writableUser(user({ source: 'nss' }), policy)).toBe(false)
  expect(writableUser(user({ system: true }), policy)).toBe(false)
  expect(writableUser(user({ system: true }), snapshot({ allow_system: true }))).toBe(true)
  expect(canDeleteUser(user(), snapshot({ control_enabled: false }))).toBe(false)
  expect(canSetPassword(user(), snapshot({ password_enabled: false }))).toBe(false)
})

test('groups need the privileged opt-in and cannot be root or nss', () => {
  expect(canDeleteGroup(group({ name: 'builders', gid: 1500, system: false }), snapshot())).toBe(
    true,
  )
  expect(canDeleteGroup(group({ gid: 0 }), snapshot({ allow_system: true }))).toBe(false)
  expect(canDeleteGroup(group({ source: 'nss' }), snapshot())).toBe(false)
  expect(canDeleteGroup(group({ name: 'wheel' }), snapshot())).toBe(false)
  expect(
    canDeleteGroup(
      group({ name: 'wheel' }),
      snapshot({ allow_privileged_groups: true, allow_system: true }),
    ),
  ).toBe(true)
  expect(canDeleteGroup(group({ system: true, name: 'users' }), snapshot())).toBe(false)
  expect(
    canDeleteGroup(group({ system: true, name: 'users' }), snapshot({ allow_system: true })),
  ).toBe(true)
})
