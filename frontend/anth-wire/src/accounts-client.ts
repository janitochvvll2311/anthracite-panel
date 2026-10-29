import type { Change } from './actions'

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

export interface AccountGroupDelete {
  name: string
}

export interface AccountSetPassword {
  name: string
  password: string
}

export interface AccountClient {
  get(): Promise<AccountSnapshot>
  createUser(request: AccountCreate): Promise<Change>
  deleteUser(request: AccountDelete): Promise<Change>
  setPassword(request: AccountSetPassword): Promise<Change>
  createGroup(request: AccountGroupCreate): Promise<Change>
  deleteGroup(request: AccountGroupDelete): Promise<Change>
}
