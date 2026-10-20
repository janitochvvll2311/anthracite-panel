import type {
  AccountGroup,
  AccountSnapshot,
  AccountSource,
  AccountUser,
} from './account-client'

export type UserSortKey = 'name' | 'uid' | 'gid' | 'group' | 'state' | 'shell' | 'source'
export type GroupSortKey = 'name' | 'gid' | 'members' | 'source'
export type SortDirection = 'asc' | 'desc'

/**
 * Groups that grant root-equivalent power. The daemon refuses to touch them
 * without an opt-in, so the UI marks them rather than letting an operator
 * discover the refusal halfway through a form.
 */
export const PRIVILEGED_GROUPS = ['sudo', 'wheel', 'admin', 'root', 'docker', 'lxd']

/** Administrative groups whose last member the daemon protects. */
export const ADMIN_GROUPS = ['sudo', 'wheel', 'admin']

export function sourceLabel(source: AccountSource): string {
  return source === 'nss' ? 'nss' : 'local'
}

/** `nss` entries come from a directory service and cannot be written back. */
export function isWritable(entry: { source: AccountSource }): boolean {
  return entry.source !== 'nss'
}

/**
 * The lock state as the table shows it. `null` is "unknown", which is the
 * honest answer when `/etc/shadow` was unreadable; it is never `unlocked`.
 */
export function stateLabel(user: AccountUser): string {
  if (user.locked === true) return 'locked'
  if (user.locked === false) return 'unlocked'
  return 'unknown'
}

export function isPrivilegedGroup(name: string): boolean {
  return PRIVILEGED_GROUPS.includes(name)
}

export function isAdministrativeGroup(name: string): boolean {
  return ADMIN_GROUPS.includes(name)
}

export function groupMembersLabel(group: AccountGroup): string {
  return group.members.length === 0 ? '—' : group.members.join(', ')
}

export function userCountLabel(snapshot: AccountSnapshot): string {
  return countLabel(snapshot.users.length, snapshot.total_users, 'user')
}

export function groupCountLabel(snapshot: AccountSnapshot): string {
  return countLabel(snapshot.groups.length, snapshot.total_groups, 'group')
}

function countLabel(shown: number, total: number, noun: string): string {
  // The total decides the plural: "1 of 2500 users" reads correctly, and
  // "1 of 2500 user" does not.
  if (total > shown) return `${shown} of ${total} ${noun}s`
  return `${shown} ${shown === 1 ? noun : `${noun}s`}`
}

/** True when the snapshot was shortened to fit the protocol line cap. */
export function wasShortened(snapshot: AccountSnapshot): boolean {
  return (
    snapshot.truncated ||
    snapshot.total_users > snapshot.users.length ||
    snapshot.total_groups > snapshot.groups.length
  )
}

/**
 * Why this entry cannot be edited, or `null` when it can. This mirrors the
 * daemon's guards so the UI can disable a row instead of offering an action
 * that is certain to be refused.
 *
 * It is a hint, not a policy: the daemon re-checks everything, because a UI
 * check is not a security boundary.
 */
export function userBlockReason(
  user: AccountUser,
  options: { allowSystem: boolean },
): string | null {
  if (!isWritable(user)) return 'Directory service account; read-only'
  if (user.uid === 0) return 'uid 0 is never changed through the panel'
  if (user.system && !options.allowSystem) {
    return 'System account; set accounts.allow_system to change it'
  }
  return null
}

/** The same hint for a group. */
export function groupBlockReason(
  group: AccountGroup,
  options: { allowSystem: boolean; allowPrivileged: boolean },
): string | null {
  if (!isWritable(group)) return 'Directory service group; read-only'
  if (group.gid === 0) return 'gid 0 is never changed through the panel'
  if (group.system && !options.allowSystem) {
    return 'System group; set accounts.allow_system to change it'
  }
  if (isPrivilegedGroup(group.name) && !options.allowPrivileged) {
    return 'Privileged group; set accounts.allow_privileged_groups to change it'
  }
  return null
}

function matches(query: string, ...fields: Array<string | number>): boolean {
  if (query === '') return true
  return fields.some((field) => String(field).toLowerCase().includes(query))
}

export function filterUsers(users: AccountUser[], filter: string): AccountUser[] {
  const query = filter.trim().toLowerCase()
  return users.filter((user) =>
    matches(query, user.name, user.uid, user.gid, user.group, user.gecos, user.shell),
  )
}

export function filterGroups(groups: AccountGroup[], filter: string): AccountGroup[] {
  const query = filter.trim().toLowerCase()
  return groups.filter((group) =>
    matches(query, group.name, group.gid, group.members.join(' ')),
  )
}

/** Numeric fields sort as numbers; everything else is compared as text. */
export function compareUsers(
  left: AccountUser,
  right: AccountUser,
  key: UserSortKey,
  direction: SortDirection,
): number {
  const sign = direction === 'asc' ? 1 : -1
  switch (key) {
    case 'uid':
    case 'gid':
      return (left[key] - right[key]) * sign
    case 'state':
      // Locked first in ascending order: it is the state that needs attention.
      return (stateRank(left) - stateRank(right)) * sign
    default:
      return String(left[key]).localeCompare(String(right[key])) * sign
  }
}

function stateRank(user: AccountUser): number {
  if (user.locked === true) return 0
  if (user.locked === null) return 1
  return 2
}

export function compareGroups(
  left: AccountGroup,
  right: AccountGroup,
  key: GroupSortKey,
  direction: SortDirection,
): number {
  const sign = direction === 'asc' ? 1 : -1
  switch (key) {
    case 'gid':
      return (left.gid - right.gid) * sign
    case 'members':
      return (left.members.length - right.members.length) * sign
    default:
      return String(left[key]).localeCompare(String(right[key])) * sign
  }
}

/**
 * Users a new member list may reference: local, so the daemon will accept them.
 * Sorted by name because the picker is a long list.
 */
export function assignableUsers(snapshot: AccountSnapshot): AccountUser[] {
  return snapshot.users
    .filter((user) => isWritable(user))
    .slice()
    .sort((left, right) => left.name.localeCompare(right.name))
}

/** Parses a member list from a comma-separated field, trimming and dropping blanks. */
export function parseMembers(value: string): string[] {
  return value
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry !== '')
}

/**
 * Parses an optional numeric field. Returns `undefined` for an empty string and
 * throws for anything that is not a non-negative integer, so a typo does not
 * silently become 0.
 */
export function parseOptionalId(value: string, label: string): number | undefined {
  const trimmed = value.trim()
  if (trimmed === '') return undefined
  if (!/^\d+$/.test(trimmed)) {
    throw new Error(`${label} must be a whole number.`)
  }
  const parsed = Number(trimmed)
  if (!Number.isSafeInteger(parsed)) {
    throw new Error(`${label} is too large.`)
  }
  return parsed
}

/** True when the form would change nothing, which the daemon rejects. */
export function isEmptyModify(request: object): boolean {
  return Object.values(request).every((value) => value === undefined)
}
