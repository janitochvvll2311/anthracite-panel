import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import './anth-fab'
import type {
  AccountClient,
  AccountGroup,
  AccountSnapshot,
  AccountUser,
} from '../lib/accounts/account-client'
import {
  assignableUsers,
  compareGroups,
  compareUsers,
  filterGroups,
  filterUsers,
  groupBlockReason,
  groupCountLabel,
  groupMembersLabel,
  isPrivilegedGroup,
  isEmptyModify,
  parseMembers,
  parseOptionalId,
  sourceLabel,
  stateLabel,
  userBlockReason,
  userCountLabel,
  wasShortened,
} from '../lib/accounts/display'
import type { GroupSortKey, SortDirection, UserSortKey } from '../lib/accounts/display'
import { confirmAction } from '../lib/ui/dialog'

type Pane = 'users' | 'groups'

/** Which editor is open, if any. */
type Editor =
  | { kind: 'create-user' }
  | { kind: 'modify-user'; user: AccountUser }
  | { kind: 'password'; user: AccountUser }
  | { kind: 'create-group' }
  | { kind: 'modify-group'; group: AccountGroup }

/**
 * Local user and group administration.
 *
 * It depends on the injected `AccountClient`; the HTTP implementation lives in
 * `lib/accounts/account-client.ts`. Refresh is manual by design: an account
 * change is an operator action, and polling would race the mutation.
 *
 * The daemon is the security boundary. This component duplicates a few guards
 * only to disable actions that would certainly fail; it never assumes its own
 * check is authoritative.
 */
@customElement('anth-account-manager')
export class AnthAccountManager extends LitElement {
  @property({ attribute: false })
  client?: AccountClient

  @property({ type: Boolean, attribute: 'auto-load' })
  autoLoad = true

  @state()
  private snapshot?: AccountSnapshot

  @state()
  private loading = false

  @state()
  private busy = false

  @state()
  private errorMessage = ''

  @state()
  private notice = ''

  @state()
  private pane: Pane = 'users'

  @state()
  private filter = ''

  @state()
  private userSort: UserSortKey = 'uid'

  @state()
  private userDirection: SortDirection = 'asc'

  @state()
  private groupSort: GroupSortKey = 'gid'

  @state()
  private groupDirection: SortDirection = 'asc'

  @state()
  private editor?: Editor

  // Create-user fields.
  @state()
  private formName = ''

  @state()
  private formUid = ''

  @state()
  private formGid = ''

  @state()
  private formGecos = ''

  @state()
  private formHome = ''

  @state()
  private formShell = ''

  @state()
  private formSystem = false

  // Group fields, shared by create and modify.
  @state()
  private formGroupName = ''

  @state()
  private formGroupGid = ''

  @state()
  private formMembers = ''

  // Password fields.
  @state()
  private password = ''

  @state()
  private passwordConfirm = ''

  @state()
  private removeHome = false

  protected override willUpdate(changed: PropertyValues<this>): void {
    if (
      this.client &&
      this.autoLoad &&
      (changed.has('client') || (changed.has('autoLoad') && this.autoLoad))
    ) {
      void this.refresh()
    }
  }

  /** Fetches a fresh inventory. Manual by design; there is no polling loop. */
  async refresh(): Promise<void> {
    if (!this.client || this.loading || this.busy) return

    this.loading = true
    this.errorMessage = ''
    try {
      this.snapshot = await this.client.get()
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.loading = false
    }
  }

  private get canControl(): boolean {
    return this.snapshot?.control_enabled === true
  }

  private get canManagePasswords(): boolean {
    return this.canControl && this.snapshot?.password_enabled === true
  }

  private get visibleUsers(): AccountUser[] {
    const users = filterUsers(this.snapshot?.users ?? [], this.filter)
    return users
      .slice()
      .sort((left, right) => compareUsers(left, right, this.userSort, this.userDirection))
  }

  private get visibleGroups(): AccountGroup[] {
    const groups = filterGroups(this.snapshot?.groups ?? [], this.filter)
    return groups
      .slice()
      .sort((left, right) => compareGroups(left, right, this.groupSort, this.groupDirection))
  }

  /** Local users a member list may reference, offered as input hints. */
  private get assignable(): AccountUser[] {
    return this.snapshot ? assignableUsers(this.snapshot) : []
  }

  private blockUser(user: AccountUser): string | null {
    return userBlockReason(user, {
      allowSystem: this.snapshot?.allow_system === true,
    })
  }

  private blockGroup(group: AccountGroup): string | null {
    return groupBlockReason(group, {
      allowSystem: this.snapshot?.allow_system === true,
      allowPrivileged: this.snapshot?.allow_privileged_groups === true,
    })
  }

  private setPane(pane: Pane): void {
    this.pane = pane
    this.filter = ''
  }

  private onFilterInput(event: Event): void {
    this.filter = (event.target as HTMLInputElement).value
  }

  private field(event: Event): string {
    return (event.target as HTMLInputElement).value
  }

  private checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked
  }

  private sortUsers(key: UserSortKey): void {
    if (this.userSort === key) {
      this.userDirection = this.userDirection === 'asc' ? 'desc' : 'asc'
      return
    }
    this.userSort = key
    this.userDirection = 'asc'
  }

  private sortGroups(key: GroupSortKey): void {
    if (this.groupSort === key) {
      this.groupDirection = this.groupDirection === 'asc' ? 'desc' : 'asc'
      return
    }
    this.groupSort = key
    this.groupDirection = 'asc'
  }

  private sortMark(current: string, key: string, direction: SortDirection): string {
    if (current !== key) return ''
    return direction === 'asc' ? ' ▲' : ' ▼'
  }

  private closeEditor(): void {
    this.editor = undefined
    this.resetForms()
  }

  private resetForms(): void {
    this.formName = ''
    this.formUid = ''
    this.formGid = ''
    this.formGecos = ''
    this.formHome = ''
    this.formShell = ''
    this.formSystem = false
    this.formGroupName = ''
    this.formGroupGid = ''
    this.formMembers = ''
    this.password = ''
    this.passwordConfirm = ''
    this.removeHome = false
    this.errorMessage = ''
  }

  private openCreateUser(): void {
    this.resetForms()
    this.editor = { kind: 'create-user' }
  }

  private openModifyUser(user: AccountUser): void {
    this.resetForms()
    this.formGecos = user.gecos
    this.formShell = user.shell
    this.formHome = user.home
    this.editor = { kind: 'modify-user', user }
  }

  private openPassword(user: AccountUser): void {
    this.resetForms()
    this.editor = { kind: 'password', user }
  }

  private openCreateGroup(): void {
    this.resetForms()
    this.editor = { kind: 'create-group' }
  }

  private openModifyGroup(group: AccountGroup): void {
    this.resetForms()
    this.formGroupGid = String(group.gid)
    this.formMembers = group.members.join(', ')
    this.editor = { kind: 'modify-group', group }
  }

  /** Runs a mutation, then refreshes so the table shows what the daemon did. */
  private async run(
    action: () => Promise<string>,
    onSuccess?: () => void,
  ): Promise<boolean> {
    if (!this.client || this.busy) return false

    this.busy = true
    this.errorMessage = ''
    this.notice = ''
    let changed = false
    try {
      this.notice = await action()
      onSuccess?.()
      changed = true
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.busy = false
    }

    if (changed) await this.refresh()
    return changed
  }

  private async submitCreateUser(event: Event): Promise<void> {
    event.preventDefault()
    if (!this.client) return

    const name = this.formName.trim()
    if (name === '') {
      this.errorMessage = 'A user name is required.'
      return
    }

    try {
      const uid = parseOptionalId(this.formUid, 'UID')
      const gid = parseOptionalId(this.formGid, 'GID')
      const gecos = this.formGecos.trim()
      const home = this.formHome.trim()
      const shell = this.formShell.trim()

      await this.run(
        () =>
          this.client!.createUser({
            name,
            uid,
            gid,
            gecos: gecos === '' ? undefined : gecos,
            home: home === '' ? undefined : home,
            shell: shell === '' ? undefined : shell,
            system: this.formSystem,
          }),
        () => this.closeEditor(),
      )
    } catch (error) {
      this.errorMessage = messageOf(error)
    }
  }

  private async submitModifyUser(event: Event, user: AccountUser): Promise<void> {
    event.preventDefault()
    if (!this.client) return

    const gecos = this.formGecos.trim()
    const home = this.formHome.trim()
    const shell = this.formShell.trim()

    const request = {
      gecos: gecos === user.gecos ? undefined : gecos,
      home: home === user.home ? undefined : home,
      shell: shell === user.shell ? undefined : shell,
    }

    if (isEmptyModify(request)) {
      this.errorMessage = 'Nothing changed.'
      return
    }

    await this.run(
      () => this.client!.modifyUser(user.name, request),
      () => this.closeEditor(),
    )
  }

  private async toggleLock(user: AccountUser): Promise<void> {
    if (!this.client) return
    const lock = user.locked !== true
    const verb = lock ? 'Lock' : 'Unlock'
    const accepted = await confirmAction({
      title: `${verb} ${user.name}?`,
      message: lock
        ? 'A locked account cannot log in.'
        : 'The account can log in again with its current password.',
      confirmLabel: verb,
      danger: lock,
    })
    if (!accepted) return
    await this.run(() => this.client!.modifyUser(user.name, { locked: lock }))
  }

  private async confirmDeleteUser(user: AccountUser): Promise<void> {
    if (!this.client) return
    const accepted = await confirmAction({
      title: `Delete ${user.name}?`,
      message: this.removeHome
        ? 'The home directory is removed with the account.'
        : undefined,
      confirmLabel: 'Delete user',
      danger: true,
    })
    if (!accepted) return
    await this.run(
      () => this.client!.deleteUser(user.name, this.removeHome),
      () => this.closeEditor(),
    )
  }

  private async submitPassword(event: Event, user: AccountUser): Promise<void> {
    event.preventDefault()
    if (!this.client) return

    if (this.password.length < 8) {
      this.errorMessage = 'A password needs at least 8 characters.'
      return
    }
    if (this.password !== this.passwordConfirm) {
      this.errorMessage = 'The two passwords do not match.'
      return
    }

    await this.run(
      () => this.client!.setPassword(user.name, this.password),
      () => this.closeEditor(),
    )
  }

  private async submitCreateGroup(event: Event): Promise<void> {
    event.preventDefault()
    if (!this.client) return

    const name = this.formGroupName.trim()
    if (name === '') {
      this.errorMessage = 'A group name is required.'
      return
    }

    try {
      const gid = parseOptionalId(this.formGroupGid, 'GID')
      await this.run(
        () => this.client!.createGroup({ name, gid, system: this.formSystem }),
        () => this.closeEditor(),
      )
    } catch (error) {
      this.errorMessage = messageOf(error)
    }
  }

  private async submitModifyGroup(event: Event, group: AccountGroup): Promise<void> {
    event.preventDefault()
    if (!this.client) return

    const original = group.members.join(',')
    const members = parseMembers(this.formMembers)
    const membersChanged = members.join(',') !== original

    try {
      const gid = parseOptionalId(this.formGroupGid, 'GID')
      const request = {
        gid: gid === group.gid ? undefined : gid,
        members: membersChanged ? members : undefined,
      }

      if (isEmptyModify(request)) {
        this.errorMessage = 'Nothing changed.'
        return
      }

      if (
        membersChanged &&
        !(await confirmAction({
          title: `Replace the members of ${group.name}?`,
          message: 'Every member not in the list is removed from the group.',
          confirmLabel: 'Replace',
          danger: true,
        }))
      ) {
        return
      }

      await this.run(
        () => this.client!.modifyGroup(group.name, request),
        () => this.closeEditor(),
      )
    } catch (error) {
      this.errorMessage = messageOf(error)
    }
  }

  private async confirmDeleteGroup(group: AccountGroup): Promise<void> {
    if (!this.client) return
    const accepted = await confirmAction({
      title: `Delete the group ${group.name}?`,
      message: 'The accounts in it are kept.',
      confirmLabel: 'Delete group',
      danger: true,
    })
    if (!accepted) return
    await this.run(() => this.client!.deleteGroup(group.name))
  }

  private renderUserEditor() {
    const editor = this.editor
    if (!editor) return nothing

    if (editor.kind === 'create-user') {
      return html`
        <form class="editor" part="editor" @submit=${this.submitCreateUser}>
          <h3>New user</h3>
          <label>
            Name
            <input
              type="text"
              autocomplete="off"
              data-field="name"
              placeholder="alice"
              .value=${this.formName}
              @input=${(event: Event) => (this.formName = this.field(event))}
            />
          </label>
          <label>
            Comment
            <input
              type="text"
              autocomplete="off"
              data-field="gecos"
              placeholder="Alice Example"
              .value=${this.formGecos}
              @input=${(event: Event) => (this.formGecos = this.field(event))}
            />
          </label>
          <label>
            UID
            <input
              type="text"
              inputmode="numeric"
              data-field="uid"
              placeholder="automatic"
              .value=${this.formUid}
              @input=${(event: Event) => (this.formUid = this.field(event))}
            />
          </label>
          <label>
            Primary GID
            <input
              type="text"
              inputmode="numeric"
              data-field="gid"
              placeholder="automatic"
              .value=${this.formGid}
              @input=${(event: Event) => (this.formGid = this.field(event))}
            />
          </label>
          <label>
            Home
            <input
              type="text"
              autocomplete="off"
              data-field="home"
              placeholder="/home/alice"
              .value=${this.formHome}
              @input=${(event: Event) => (this.formHome = this.field(event))}
            />
          </label>
          <label>
            Shell
            <input
              type="text"
              autocomplete="off"
              data-field="shell"
              placeholder="/bin/sh"
              .value=${this.formShell}
              @input=${(event: Event) => (this.formShell = this.field(event))}
            />
          </label>
          ${this.snapshot?.allow_system
            ? html`
                <label class="check">
                  <input
                    type="checkbox"
                    .checked=${this.formSystem}
                    @change=${(event: Event) => (this.formSystem = this.checked(event))}
                  />
                  System account
                </label>
              `
            : nothing}
          <p class="hint">
            The account is created locked, so it cannot log in until a password
            is set.
          </p>
          <div class="editor-actions">
            <button type="submit" ?disabled=${this.busy}>Create user</button>
            <button type="button" @click=${this.closeEditor}>Cancel</button>
          </div>
        </form>
      `
    }

    if (editor.kind === 'modify-user') {
      const user = editor.user
      return html`
        <form class="editor" part="editor" @submit=${(event: Event) => this.submitModifyUser(event, user)}>
          <h3>Edit ${user.name}</h3>
          <label>
            Comment
            <input
              type="text"
              autocomplete="off"
              data-field="gecos"
              .value=${this.formGecos}
              @input=${(event: Event) => (this.formGecos = this.field(event))}
            />
          </label>
          <label>
            Home
            <input
              type="text"
              autocomplete="off"
              data-field="home"
              .value=${this.formHome}
              @input=${(event: Event) => (this.formHome = this.field(event))}
            />
          </label>
          <label>
            Shell
            <input
              type="text"
              autocomplete="off"
              data-field="shell"
              .value=${this.formShell}
              @input=${(event: Event) => (this.formShell = this.field(event))}
            />
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.removeHome}
              @change=${(event: Event) => (this.removeHome = this.checked(event))}
            />
            Also remove the home directory when deleting
          </label>
          <div class="editor-actions">
            <button type="submit" ?disabled=${this.busy}>Save changes</button>
            <button type="button" ?disabled=${this.busy} @click=${() => this.toggleLock(user)}>
              ${user.locked === true ? 'Unlock' : 'Lock'}
            </button>
            <button type="button" class="danger" ?disabled=${this.busy} @click=${() => this.confirmDeleteUser(user)}>
              Delete
            </button>
            <button type="button" @click=${this.closeEditor}>Cancel</button>
          </div>
        </form>
      `
    }

    if (editor.kind === 'password') {
      const user = editor.user
      return html`
        <form class="editor" part="editor" @submit=${(event: Event) => this.submitPassword(event, user)}>
          <h3>Password for ${user.name}</h3>
          <label>
            New password
            <input
              type="password"
              autocomplete="new-password"
              .value=${this.password}
              @input=${(event: Event) => (this.password = this.field(event))}
            />
          </label>
          <label>
            Repeat
            <input
              type="password"
              autocomplete="new-password"
              .value=${this.passwordConfirm}
              @input=${(event: Event) => (this.passwordConfirm = this.field(event))}
            />
          </label>
          <p class="hint">
            The password travels in the request body. Make sure the panel is on a
            trusted network.
          </p>
          <div class="editor-actions">
            <button type="submit" ?disabled=${this.busy}>Set password</button>
            <button type="button" @click=${this.closeEditor}>Cancel</button>
          </div>
        </form>
      `
    }

    if (editor.kind === 'create-group') {
      return html`
        <form class="editor" part="editor" @submit=${this.submitCreateGroup}>
          <h3>New group</h3>
          <label>
            Name
            <input
              type="text"
              autocomplete="off"
              data-field="group-name"
              placeholder="developers"
              .value=${this.formGroupName}
              @input=${(event: Event) => (this.formGroupName = this.field(event))}
            />
          </label>
          <label>
            GID
            <input
              type="text"
              inputmode="numeric"
              data-field="gid"
              placeholder="automatic"
              .value=${this.formGroupGid}
              @input=${(event: Event) => (this.formGroupGid = this.field(event))}
            />
          </label>
          ${this.snapshot?.allow_system
            ? html`
                <label class="check">
                  <input
                    type="checkbox"
                    .checked=${this.formSystem}
                    @change=${(event: Event) => (this.formSystem = this.checked(event))}
                  />
                  System group
                </label>
              `
            : nothing}
          <div class="editor-actions">
            <button type="submit" ?disabled=${this.busy}>Create group</button>
            <button type="button" @click=${this.closeEditor}>Cancel</button>
          </div>
        </form>
      `
    }

    const group = editor.group
    return html`
      <form class="editor" part="editor" @submit=${(event: Event) => this.submitModifyGroup(event, group)}>
        <h3>Edit ${group.name}</h3>
        <label>
          GID
          <input
            type="text"
            inputmode="numeric"
            data-field="gid"
            .value=${this.formGroupGid}
            @input=${(event: Event) => (this.formGroupGid = this.field(event))}
          />
        </label>
        <label>
          Members
          <input
            type="text"
            autocomplete="off"
            data-field="members"
            list="account-member-hints"
            placeholder="alice, bob"
            .value=${this.formMembers}
            @input=${(event: Event) => (this.formMembers = this.field(event))}
          />
          <datalist id="account-member-hints">
            ${this.assignable.map((user) => html`<option value=${user.name}></option>`)}
          </datalist>
        </label>
        <p class="hint">
          This list replaces the current members. Leave it empty to remove
          everybody.
        </p>
        <div class="editor-actions">
          <button type="submit" ?disabled=${this.busy}>Save changes</button>
          <button type="button" class="danger" ?disabled=${this.busy} @click=${() => this.confirmDeleteGroup(group)}>
            Delete
          </button>
          <button type="button" @click=${this.closeEditor}>Cancel</button>
        </div>
      </form>
    `
  }

  private renderUsers() {
    const users = this.visibleUsers
    if (users.length === 0) {
      return html`<p class="empty">
        ${this.snapshot === undefined
          ? this.loading
            ? 'Loading accounts…'
            : 'No account snapshot yet.'
          : 'No users match the filter.'}
      </p>`
    }

    return html`
      <div class="table-wrap">
        <table>
          <caption>
            Users
            <span class="muted">${userCountLabel(this.snapshot!)}</span>
          </caption>
          <thead>
            <tr>
              ${this.columnHeader('User', 'name', this.userSort, this.userDirection, (key) =>
                this.sortUsers(key),
              )}
              ${this.columnHeader('UID', 'uid', this.userSort, this.userDirection, (key) =>
                this.sortUsers(key),
              )}
              ${this.columnHeader('Group', 'group', this.userSort, this.userDirection, (key) =>
                this.sortUsers(key),
              )}
              ${this.columnHeader('State', 'state', this.userSort, this.userDirection, (key) =>
                this.sortUsers(key),
              )}
              ${this.columnHeader('Shell', 'shell', this.userSort, this.userDirection, (key) =>
                this.sortUsers(key),
              )}
              ${this.columnHeader('Source', 'source', this.userSort, this.userDirection, (key) =>
                this.sortUsers(key),
              )}
              <th class="actions-head">Control</th>
            </tr>
          </thead>
          <tbody>
            ${users.map((user) => {
              const blocked = this.blockUser(user)
              return html`
                <tr data-blocked=${blocked !== null}>
                  <td class="mono" title=${user.gecos}>
                    <span class="name">${user.name}</span>
                    ${user.system
                      ? html`<span class="tag" title="System account">sys</span>`
                      : nothing}
                  </td>
                  <td class="mono">${user.uid}</td>
                  <td class="mono">${user.group}</td>
                  <td class="mono state" data-state=${stateLabel(user)}>${stateLabel(user)}</td>
                  <td class="mono" title=${user.shell}>${user.shell}</td>
                  <td class="mono source" title=${blocked ?? ''}>${sourceLabel(user.source)}</td>
                  <td class="actions">
                    ${this.canControl && blocked === null
                      ? html`
                          <button
                            type="button"
                            ?disabled=${this.busy}
                            @click=${() => this.openModifyUser(user)}
                          >
                            Edit
                          </button>
                          ${this.canManagePasswords
                            ? html`
                                <button
                                  type="button"
                                  ?disabled=${this.busy}
                                  @click=${() => this.openPassword(user)}
                                >
                                  Password
                                </button>
                              `
                            : nothing}
                        `
                      : html`<span class="muted reason" title=${blocked ?? ''}>
                          ${blocked ?? ''}
                        </span>`}
                  </td>
                </tr>
              `
            })}
          </tbody>
        </table>
      </div>
    `
  }

  private renderGroups() {
    const groups = this.visibleGroups
    if (groups.length === 0) {
      return html`<p class="empty">
        ${this.snapshot === undefined
          ? 'No groups yet.'
          : 'No groups match the filter.'}
      </p>`
    }

    return html`
      <div class="table-wrap">
        <table>
          <caption>
            Groups
            <span class="muted">${groupCountLabel(this.snapshot!)}</span>
          </caption>
          <thead>
            <tr>
              ${this.columnHeader('Group', 'name', this.groupSort, this.groupDirection, (key) =>
                this.sortGroups(key),
              )}
              ${this.columnHeader('GID', 'gid', this.groupSort, this.groupDirection, (key) =>
                this.sortGroups(key),
              )}
              ${this.columnHeader('Members', 'members', this.groupSort, this.groupDirection, (key) =>
                this.sortGroups(key),
              )}
              ${this.columnHeader('Source', 'source', this.groupSort, this.groupDirection, (key) =>
                this.sortGroups(key),
              )}
              <th class="actions-head">Control</th>
            </tr>
          </thead>
          <tbody>
            ${groups.map((group) => {
              const blocked = this.blockGroup(group)
              return html`
                <tr data-blocked=${blocked !== null}>
                  <td class="mono">
                    <span class="name">${group.name}</span>
                    ${group.system
                      ? html`<span class="tag" title="System group">sys</span>`
                      : nothing}
                    ${isPrivilegedGroup(group.name)
                      ? html`<span class="tag danger-tag" title="Grants root-equivalent power"
                          >admin</span
                        >`
                      : nothing}
                  </td>
                  <td class="mono">${group.gid}</td>
                  <td class="mono members" title=${groupMembersLabel(group)}>
                    ${groupMembersLabel(group)}
                  </td>
                  <td class="mono source" title=${blocked ?? ''}>${sourceLabel(group.source)}</td>
                  <td class="actions">
                    ${this.canControl && blocked === null
                      ? html`
                          <button
                            type="button"
                            ?disabled=${this.busy}
                            @click=${() => this.openModifyGroup(group)}
                          >
                            Edit
                          </button>
                        `
                      : html`<span class="muted reason" title=${blocked ?? ''}>
                          ${blocked ?? ''}
                        </span>`}
                  </td>
                </tr>
              `
            })}
          </tbody>
        </table>
      </div>
    `
  }

  private columnHeader<K extends string>(
    label: string,
    key: K,
    current: string,
    direction: SortDirection,
    onSort: (key: K) => void,
  ) {
    return html`
      <th>
        <button type="button" class="sort" @click=${() => onSort(key)}>
          ${label}${this.sortMark(current, key, direction)}
        </button>
      </th>
    `
  }

  override render() {
    const snapshot = this.snapshot
    const shortened = snapshot !== undefined && wasShortened(snapshot)

    return html`
      <section class="accounts" part="accounts" aria-busy=${this.loading || this.busy}>
        <header class="toolbar" part="toolbar">
          <div class="status">
            <span class="dot" data-ready=${snapshot !== undefined}></span>
            <span>${snapshot ? 'inventory loaded' : 'no snapshot'}</span>
            ${snapshot
              ? html`<span class="muted">
                  ${userCountLabel(snapshot)} · ${groupCountLabel(snapshot)}
                </span>`
              : nothing}
          </div>
          <span class="spacer"></span>
          <div class="tabs" role="tablist">
            <button
              type="button"
              role="tab"
              aria-selected=${this.pane === 'users'}
              data-active=${this.pane === 'users'}
              @click=${() => this.setPane('users')}
            >
              Users
            </button>
            <button
              type="button"
              role="tab"
              aria-selected=${this.pane === 'groups'}
              data-active=${this.pane === 'groups'}
              @click=${() => this.setPane('groups')}
            >
              Groups
            </button>
          </div>
          <input
            type="search"
            class="filter"
            placeholder="Filter"
            aria-label="Filter users and groups"
            .value=${this.filter}
            @input=${this.onFilterInput}
          />
          <button
            type="button"
            @click=${this.refresh}
            ?disabled=${!this.client || this.loading || this.busy}
          >
            ${this.loading ? 'Loading…' : 'Refresh'}
          </button>
          ${this.canControl
            ? html`
                <button
                  type="button"
                  class="primary"
                  ?disabled=${this.busy}
                  @click=${this.pane === 'users' ? this.openCreateUser : this.openCreateGroup}
                >
                  ${this.pane === 'users' ? 'New user' : 'New group'}
                </button>
              `
            : nothing}
        </header>

        ${this.errorMessage
          ? html`<p class="error" part="error" role="alert">${this.errorMessage}</p>`
          : nothing}
        ${this.notice
          ? html`<p class="notice" part="notice" role="status">${this.notice}</p>`
          : nothing}
        ${shortened
          ? html`<p class="warning" part="warning" role="status">
              ${snapshot?.warning ?? 'The snapshot was shortened to fit the protocol limit.'}
            </p>`
          : nothing}
        ${this.canControl
          ? nothing
          : html`<p class="control-off">
              Account control is disabled by the daemon. The inventory stays
              read-only.
            </p>`}

        ${this.renderUserEditor()} ${this.pane === 'users' ? this.renderUsers() : this.renderGroups()}

        ${this.canControl
          ? html`
              <anth-fab
                icon="＋"
                label=${this.pane === 'users' ? 'New user' : 'New group'}
                ?disabled=${this.busy}
                @click=${this.pane === 'users'
                  ? this.openCreateUser
                  : this.openCreateGroup}
              ></anth-fab>
            `
          : nothing}
      </section>
    `
  }

  static styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 14px/1.45 var(--sans, system-ui, sans-serif);
    }

    * {
      box-sizing: border-box;
    }

    .accounts {
      display: flex;
      flex-direction: column;
      gap: 10px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      padding: 12px;
    }

    .toolbar,
    .status,
    .tabs,
    .editor-actions {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
    }

    .toolbar {
      justify-content: space-between;
    }

    .spacer {
      flex: 1;
    }

    .status {
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .muted {
      color: var(--muted, #8b9bad);
    }

    .reason {
      font: 11px/1.4 var(--mono, monospace);
    }

    .dot {
      width: 7px;
      height: 7px;
      border-radius: 50%;
      background: #64748b;
    }

    .dot[data-ready='true'] {
      background: #34d399;
    }

    button,
    select,
    input {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      padding: 4px 9px;
      font: inherit;
    }

    button {
      cursor: pointer;
    }

    button:disabled {
      opacity: 0.45;
      cursor: default;
    }

    button:hover:not(:disabled) {
      border-color: var(--accent, #7dd3fc);
    }

    button:focus-visible,
    input:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .tabs button[data-active='true'] {
      border-color: var(--accent, #7dd3fc);
      color: var(--accent, #7dd3fc);
    }

    .primary {
      border-color: var(--accent, #7dd3fc);
    }

    .danger {
      color: #fca5a5;
    }

    .filter {
      min-width: 10rem;
    }

    .error,
    .notice,
    .warning,
    .control-off {
      margin: 0;
      padding: 8px 10px;
      border-radius: 6px;
    }

    .error {
      background: rgba(248, 113, 113, 0.12);
      color: #fecaca;
    }

    .warning {
      background: rgba(251, 191, 36, 0.12);
      color: #fcd34d;
    }

    .notice {
      background: rgba(52, 211, 153, 0.12);
      color: #a7f3d0;
    }

    .control-off {
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .empty {
      margin: 0;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .editor {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr));
      gap: 8px 10px;
      padding: 10px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: #0c131c;
    }

    .editor h3 {
      grid-column: 1 / -1;
      margin: 0;
      font-size: 13px;
      font-weight: 600;
    }

    .editor label {
      display: flex;
      flex-direction: column;
      gap: 3px;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }

    .editor label.check {
      flex-direction: row;
      align-items: center;
      gap: 6px;
      text-transform: none;
      letter-spacing: 0;
    }

    .editor .hint {
      grid-column: 1 / -1;
      margin: 0;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      text-transform: none;
      letter-spacing: 0;
    }

    .editor-actions {
      grid-column: 1 / -1;
    }

    .table-wrap {
      overflow: auto;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
    }

    table {
      width: 100%;
      min-width: 46rem;
      table-layout: fixed;
      border-collapse: collapse;
      white-space: nowrap;
      font: 13px/1.3 var(--mono, monospace);
    }

    caption {
      display: flex;
      justify-content: space-between;
      padding: 7px 9px;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    th,
    td {
      padding: 5px 9px;
      border-bottom: 1px solid rgba(34, 48, 65, 0.55);
      text-align: left;
      vertical-align: middle;
    }

    td {
      overflow: hidden;
      text-overflow: ellipsis;
    }

    th .sort {
      border: none;
      background: none;
      padding: 0;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    th .sort:hover:not(:disabled) {
      color: var(--text, #dbe6f2);
    }

    tbody tr:hover {
      background: rgba(125, 211, 252, 0.05);
    }

    tbody tr[data-blocked='true'] {
      opacity: 0.62;
    }

    .mono {
      font-family: var(--mono, monospace);
    }

    .name {
      color: var(--text, #dbe6f2);
    }

    .state[data-state='locked'] {
      color: #fcd34d;
    }

    .state[data-state='unknown'] {
      color: var(--muted, #8b9bad);
    }

    .tag {
      margin-left: 6px;
      padding: 0 4px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 4px;
      color: var(--muted, #8b9bad);
      font-size: 10px;
      text-transform: uppercase;
    }

    .danger-tag {
      border-color: rgba(248, 113, 113, 0.4);
      color: #fca5a5;
    }

    .actions {
      text-align: right;
    }

    .actions-head {
      text-align: right;
    }
  `
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-account-manager': AnthAccountManager
  }
}
