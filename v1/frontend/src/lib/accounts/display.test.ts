import { describe, expect, it } from 'vitest'
import {
  assignableUsers,
  compareGroups,
  compareUsers,
  filterGroups,
  filterUsers,
  groupBlockReason,
  groupCountLabel,
  groupMembersLabel,
  isAdministrativeGroup,
  isEmptyModify,
  isPrivilegedGroup,
  isWritable,
  parseMembers,
  parseOptionalId,
  sourceLabel,
  stateLabel,
  userBlockReason,
  userCountLabel,
  wasShortened,
} from './display'
import { accountGroup, accountSnapshot, accountUser } from '../../test/fake-account-client'

describe('stateLabel', () => {
  it('reports locked and unlocked from a known state', () => {
    expect(stateLabel(accountUser({ locked: true }))).toBe('locked')
    expect(stateLabel(accountUser({ locked: false }))).toBe('unlocked')
  })

  it('never guesses unlocked when the state is unknown', () => {
    // This is the security-relevant case: `null` means the daemon could not
    // read the shadow entry, and guessing would misreport a locked account.
    expect(stateLabel(accountUser({ locked: null }))).toBe('unknown')
  })
})

describe('source helpers', () => {
  it('labels the two sources', () => {
    expect(sourceLabel('local')).toBe('local')
    expect(sourceLabel('nss')).toBe('nss')
  })

  it('treats only local entries as writable', () => {
    expect(isWritable({ source: 'local' })).toBe(true)
    expect(isWritable({ source: 'nss' })).toBe(false)
  })
})

describe('privileged and administrative groups', () => {
  it('knows the root-equivalent groups', () => {
    for (const name of ['sudo', 'wheel', 'admin', 'root', 'docker', 'lxd']) {
      expect(isPrivilegedGroup(name)).toBe(true)
    }
    expect(isPrivilegedGroup('developers')).toBe(false)
  })

  it('separates the administrative groups from the rest', () => {
    expect(isAdministrativeGroup('sudo')).toBe(true)
    expect(isAdministrativeGroup('wheel')).toBe(true)
    expect(isAdministrativeGroup('admin')).toBe(true)
    // docker grants root in practice, but it is not an admin login group.
    expect(isAdministrativeGroup('docker')).toBe(false)
  })
})

describe('userBlockReason', () => {
  it('allows an ordinary local user', () => {
    expect(userBlockReason(accountUser(), { allowSystem: false })).toBeNull()
  })

  it('refuses a directory-service entry', () => {
    const reason = userBlockReason(accountUser({ source: 'nss' }), { allowSystem: true })
    expect(reason).toContain('read-only')
  })

  it('refuses uid 0 even when system accounts are allowed', () => {
    const reason = userBlockReason(accountUser({ uid: 0, system: true }), {
      allowSystem: true,
    })
    expect(reason).toContain('uid 0')
  })

  it('refuses a system account unless the policy allows it', () => {
    expect(userBlockReason(accountUser({ system: true }), { allowSystem: false })).toContain(
      'allow_system',
    )
    expect(userBlockReason(accountUser({ system: true }), { allowSystem: true })).toBeNull()
  })
})

describe('groupBlockReason', () => {
  it('refuses a privileged group unless the policy allows it', () => {
    const reason = groupBlockReason(accountGroup({ name: 'sudo' }), {
      allowSystem: false,
      allowPrivileged: false,
    })
    expect(reason).toContain('allow_privileged_groups')
  })

  it('allows a privileged group once the policy permits it', () => {
    expect(
      groupBlockReason(accountGroup({ name: 'sudo' }), {
        allowSystem: false,
        allowPrivileged: true,
      }),
    ).toBeNull()
  })

  it('reports the root warning first when a group trips both switches', () => {
    // `sudo` is below GID_MIN *and* root-equivalent. Naming allow_system first
    // would send the operator to the wrong switch.
    const reason = groupBlockReason(accountGroup({ name: 'sudo', gid: 27, system: true }), {
      allowSystem: false,
      allowPrivileged: false,
    })
    expect(reason).toContain('allow_privileged_groups')
  })

  it('falls through to the system warning once the privileged switch is on', () => {
    const reason = groupBlockReason(accountGroup({ name: 'sudo', gid: 27, system: true }), {
      allowSystem: false,
      allowPrivileged: true,
    })
    expect(reason).toContain('allow_system')
  })

  it('refuses gid 0 regardless of the policy', () => {
    const reason = groupBlockReason(accountGroup({ name: 'root', gid: 0 }), {
      allowSystem: true,
      allowPrivileged: true,
    })
    expect(reason).toContain('gid 0')
  })
})

describe('filtering', () => {
  const users = [
    accountUser({ name: 'alice', gecos: 'Alice Example' }),
    accountUser({
      name: 'bob',
      uid: 1002,
      gid: 1002,
      group: 'bob',
      gecos: 'Bob',
      shell: '/usr/sbin/nologin',
    }),
  ]

  it('returns everything for an empty filter', () => {
    expect(filterUsers(users, '   ')).toHaveLength(2)
  })

  it('matches name, comment and numeric ids', () => {
    expect(filterUsers(users, 'alice')).toHaveLength(1)
    expect(filterUsers(users, 'example')).toHaveLength(1)
    expect(filterUsers(users, '1002')).toHaveLength(1)
  })

  it('matches group members', () => {
    const groups = [accountGroup({ members: ['alice'] }), accountGroup({ name: 'ops', gid: 3 })]
    expect(filterGroups(groups, 'alice')).toHaveLength(1)
  })
})

describe('sorting', () => {
  it('sorts numeric ids as numbers, not text', () => {
    const low = accountUser({ name: 'a', uid: 9 })
    const high = accountUser({ name: 'b', uid: 100 })
    expect(compareUsers(low, high, 'uid', 'asc')).toBeLessThan(0)
    expect(compareUsers(low, high, 'uid', 'desc')).toBeGreaterThan(0)
  })

  it('puts locked accounts first in ascending order', () => {
    const locked = accountUser({ name: 'a', locked: true })
    const unknown = accountUser({ name: 'b', locked: null })
    const unlocked = accountUser({ name: 'c', locked: false })
    const sorted = [unlocked, unknown, locked].sort((l, r) => compareUsers(l, r, 'state', 'asc'))
    expect(sorted.map((user) => user.name)).toEqual(['a', 'b', 'c'])
  })

  it('sorts groups by member count', () => {
    const few = accountGroup({ name: 'a', members: ['x'] })
    const many = accountGroup({ name: 'b', members: ['x', 'y'] })
    expect(compareGroups(few, many, 'members', 'asc')).toBeLessThan(0)
  })
})

describe('labels', () => {
  it('shows a dash for a group with no members', () => {
    expect(groupMembersLabel(accountGroup({ members: [] }))).toBe('—')
    expect(groupMembersLabel(accountGroup({ members: ['a', 'b'] }))).toBe('a, b')
  })

  it('says shown of total when the snapshot was shortened', () => {
    const snapshot = accountSnapshot({ users: [accountUser()], total_users: 2500 })
    expect(userCountLabel(snapshot)).toBe('1 of 2500 users')
    expect(groupCountLabel(snapshot)).toBe('1 group')
  })

  it('detects shortening from either the flag or the counts', () => {
    expect(wasShortened(accountSnapshot({ truncated: true }))).toBe(true)
    expect(wasShortened(accountSnapshot({ total_users: 9 }))).toBe(true)
    expect(wasShortened(accountSnapshot())).toBe(false)
  })
})

describe('assignableUsers', () => {
  it('offers only local users, sorted by name', () => {
    const snapshot = accountSnapshot({
      users: [
        accountUser({ name: 'zoe' }),
        accountUser({ name: 'dana', source: 'nss' }),
        accountUser({ name: 'amy' }),
      ],
    })
    expect(assignableUsers(snapshot).map((user) => user.name)).toEqual(['amy', 'zoe'])
  })
})

describe('form parsing', () => {
  it('trims and drops blank members', () => {
    expect(parseMembers(' alice , bob ,, ')).toEqual(['alice', 'bob'])
    expect(parseMembers('')).toEqual([])
  })

  it('treats an empty id as absent', () => {
    expect(parseOptionalId('  ', 'UID')).toBeUndefined()
  })

  it('parses a numeric id', () => {
    expect(parseOptionalId('1500', 'UID')).toBe(1500)
  })

  it('rejects a typo instead of silently becoming zero', () => {
    expect(() => parseOptionalId('12abc', 'UID')).toThrow('UID must be a whole number.')
    expect(() => parseOptionalId('-5', 'UID')).toThrow('UID must be a whole number.')
  })

  it('detects a modify that would change nothing', () => {
    expect(isEmptyModify({ gecos: undefined, shell: undefined })).toBe(true)
    expect(isEmptyModify({ gecos: 'x', shell: undefined })).toBe(false)
  })
})
