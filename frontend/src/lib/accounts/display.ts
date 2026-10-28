import type { AccountGroup, AccountSnapshot, AccountUser } from '../../accounts-client'

/** Groups that grant root-equivalent power, mirrored from the daemon. */
export const PRIVILEGED_GROUPS = ['sudo', 'wheel', 'admin', 'root', 'docker', 'lxd']

export function isPrivilegedGroup(name: string): boolean {
  return PRIVILEGED_GROUPS.includes(name)
}

function matches(query: string, ...values: Array<string | number | undefined>): boolean {
  const needle = query.trim().toLowerCase()
  if (needle === '') return true
  return values.some((value) => String(value ?? '').toLowerCase().includes(needle))
}

export function filterUsers(users: AccountUser[], query: string): AccountUser[] {
  return users.filter((user) =>
    matches(
      query,
      user.name,
      user.uid,
      user.gid,
      user.group,
      user.gecos,
      user.home,
      user.shell,
      ...user.groups,
    ),
  )
}

export function filterGroups(groups: AccountGroup[], query: string): AccountGroup[] {
  return groups.filter((group) =>
    matches(query, group.name, group.gid, ...group.members),
  )
}

/** A local, non-root account the panel may change. */
export function writableUser(user: AccountUser, snapshot: AccountSnapshot): boolean {
  return (
    snapshot.control_enabled &&
    user.source === 'local' &&
    user.uid !== 0 &&
    (!user.system || snapshot.allow_system)
  )
}

export function canDeleteUser(user: AccountUser, snapshot: AccountSnapshot): boolean {
  return writableUser(user, snapshot)
}

export function canSetPassword(user: AccountUser, snapshot: AccountSnapshot): boolean {
  return snapshot.password_enabled && writableUser(user, snapshot)
}

/** A local, non-root group the panel may delete. */
export function canDeleteGroup(group: AccountGroup, snapshot: AccountSnapshot): boolean {
  return (
    snapshot.control_enabled &&
    group.source === 'local' &&
    group.gid !== 0 &&
    (!group.system || snapshot.allow_system) &&
    (!isPrivilegedGroup(group.name) || snapshot.allow_privileged_groups)
  )
}
