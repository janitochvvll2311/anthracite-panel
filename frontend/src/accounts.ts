/**
 * `anth-accounts`: the users and groups inventory.
 *
 * The component is mounted twice by the Accounts section: once with
 * `view="users"` and once with `view="groups"`. Both ask `/api/accounts` for the
 * same snapshot; mutations reload it, so a create or delete is visible at once.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import type {
  AccountCreate,
  AccountClient,
  AccountGroup,
  AccountGroupCreate,
  AccountSnapshot,
  AccountUser,
} from './accounts-client'
import { describeError } from './errors'
import {
  canDeleteGroup,
  canDeleteUser,
  canSetPassword,
  filterGroups,
  filterUsers,
} from './lib/accounts/display'

type AccountsView = 'users' | 'groups'
type LoadStatus = 'idle' | 'loading' | 'ready' | 'error'

@customElement('anth-accounts')
@localized()
export class AnthAccounts extends LitElement {
  @property({ attribute: false })
  client?: AccountClient

  @property({ type: String })
  view: AccountsView = 'users'

  @state()
  private snapshot?: AccountSnapshot

  @state()
  private status: LoadStatus = 'idle'

  @state()
  private loading = false

  @state()
  private error = ''

  @state()
  private query = ''

  @state()
  private busy = false

  @state()
  private actionError = ''

  @state()
  private notice = ''

  @state()
  private passwordFor?: string

  @state()
  private confirmingDelete?: string

  @state()
  private removeHome = false

  override connectedCallback(): void {
    super.connectedCallback()
    void this.load()
  }

  /** Re-reads the inventory now; the toolbar button calls it. */
  async load(): Promise<void> {
    const client = this.client
    if (!client || this.loading) return
    this.loading = true
    if (!this.snapshot) this.status = 'loading'
    try {
      this.snapshot = await client.get()
      this.status = 'ready'
      this.error = ''
      this.actionError = ''
      this.notice = ''
    } catch (failure) {
      this.error = describeError(failure)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private async run(
    task: (client: AccountClient) => Promise<unknown>,
    done?: () => void,
  ): Promise<void> {
    const client = this.client
    if (!client || this.busy) return
    this.busy = true
    this.actionError = ''
    this.notice = ''
    try {
      await task(client)
      done?.()
      this.snapshot = await client.get()
      this.status = 'ready'
      this.error = ''
      this.notice = msg('Change applied.')
    } catch (failure) {
      this.actionError = describeError(failure)
    } finally {
      this.busy = false
    }
  }

  private optionalNumber(value: FormDataEntryValue | null): number | undefined {
    const text = String(value ?? '').trim()
    if (text === '') return undefined
    const number = Number(text)
    return Number.isSafeInteger(number) && number >= 0 ? number : undefined
  }

  private submitCreateUser(event: Event): void {
    event.preventDefault()
    if (this.busy) return
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const name = String(data.get('name') ?? '').trim()
    if (name === '') {
      this.actionError = msg('A login name is required.')
      return
    }
    const request: AccountCreate = {
      name,
      system: data.get('system') === 'on',
    }
    const uid = this.optionalNumber(data.get('uid'))
    const gid = this.optionalNumber(data.get('gid'))
    if (uid !== undefined) request.uid = uid
    if (gid !== undefined) request.gid = gid
    for (const [field, key] of [
      ['gecos', 'gecos'],
      ['home', 'home'],
      ['shell', 'shell'],
    ] as const) {
      const value = String(data.get(field) ?? '').trim()
      if (value !== '') request[key] = value
    }
    void this.run(
      (client) => client.createUser(request),
      () => {
        form.reset()
      },
    )
  }

  private submitCreateGroup(event: Event): void {
    event.preventDefault()
    if (this.busy) return
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const name = String(data.get('name') ?? '').trim()
    if (name === '') {
      this.actionError = msg('A group name is required.')
      return
    }
    const request: AccountGroupCreate = {
      name,
      system: data.get('system') === 'on',
    }
    const gid = this.optionalNumber(data.get('gid'))
    if (gid !== undefined) request.gid = gid
    void this.run(
      (client) => client.createGroup(request),
      () => {
        form.reset()
      },
    )
  }

  private submitPassword(event: Event, user: AccountUser): void {
    event.preventDefault()
    if (this.busy) return
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const password = String(data.get('password') ?? '')
    const confirm = String(data.get('confirm') ?? '')
    if (password.length < 8) {
      this.actionError = msg('The password must have at least 8 characters.')
      return
    }
    if (password !== confirm) {
      this.actionError = msg('The passwords do not match.')
      return
    }
    void this.run(
      (client) => client.setPassword(user.name, password),
      () => {
        this.passwordFor = undefined
      },
    )
  }

  private submitDeleteUser(user: AccountUser): void {
    void this.run(
      (client) =>
        client.deleteUser({
          name: user.name,
          remove_home: this.removeHome,
        }),
      () => {
        this.confirmingDelete = undefined
        this.removeHome = false
      },
    )
  }

  private submitDeleteGroup(group: AccountGroup): void {
    void this.run(
      (client) => client.deleteGroup(group.name),
      () => {
        this.confirmingDelete = undefined
      },
    )
  }

  private statusLabel(): string {
    if (this.loading) return msg('loading')
    if (this.status === 'error') return msg('error')
    if (!this.snapshot) return msg('idle')
    return msg('ready')
  }

  private countLabel(): string {
    if (!this.snapshot) return ''
    if (this.view === 'groups') {
      const count = this.snapshot.groups.length
      const system = this.snapshot.groups.filter((group) => group.system).length
      return count === 1
        ? msg(str`1 group · ${system} system`)
        : msg(str`${count} groups · ${system} system`)
    }
    const count = this.snapshot.users.length
    const system = this.snapshot.users.filter((user) => user.system).length
    return count === 1
      ? msg(str`1 user · ${system} system`)
      : msg(str`${count} users · ${system} system`)
  }

  private renderToolbar() {
    return html`
      <div class="toolbar" part="toolbar">
        <span class="status" data-state=${this.status} role="status">
          ${this.statusLabel()}
        </span>
        <span class="count">${this.countLabel()}</span>
        <span class="spacer"></span>
        <button
          type="button"
          ?disabled=${this.loading}
          title=${msg('Refresh')}
          @click=${() => void this.load()}
        >
          <anth-icon name="refresh"></anth-icon>
          <span class="btn-label">${msg('Refresh')}</span>
        </button>
      </div>
    `
  }

  private policy(label: string, enabled: boolean) {
    return html`
      <span class="policy" data-on=${enabled ? 'yes' : 'no'}>${label}</span>
    `
  }

  private renderContext(snapshot: AccountSnapshot) {
    return html`
      <div class="context" part="context">
        <span class="context-label">${msg('Policy')}</span>
        <span class="policies">
          ${this.policy(msg('control'), snapshot.control_enabled)}
          ${this.policy(msg('passwords'), snapshot.password_enabled)}
          ${this.policy(msg('system'), snapshot.allow_system)}
          ${this.policy(msg('privileged groups'), snapshot.allow_privileged_groups)}
        </span>
      </div>
    `
  }

  private renderFilter(placeholder: string) {
    return html`
      <div class="filters" part="filters">
        <input
          type="search"
          .value=${this.query}
          placeholder=${placeholder}
          aria-label=${placeholder}
          @input=${(event: Event) => {
            this.query = (event.currentTarget as HTMLInputElement).value
          }}
        />
      </div>
    `
  }

  private badge(label: string, tone = '') {
    return html`<span class="badge" data-tone=${tone}>${label}</span>`
  }

  private renderCreateUser(snapshot: AccountSnapshot) {
    if (!snapshot.control_enabled) {
      return html`<p class="hint">${msg('Account changes are disabled; set accounts.control to enable them.')}</p>`
    }
    return html`
      <form
        class="create"
        part="create-user"
        @submit=${(event: Event) => this.submitCreateUser(event)}
      >
        <label>
          <span>${msg('Name')}</span>
          <input name="name" autocomplete="off" ?disabled=${this.busy} />
        </label>
        <label>
          <span>${msg('UID (optional)')}</span>
          <input name="uid" type="number" min="1" ?disabled=${this.busy} />
        </label>
        <label>
          <span>${msg('GID (optional)')}</span>
          <input name="gid" type="number" min="1" ?disabled=${this.busy} />
        </label>
        <label>
          <span>${msg('Description (optional)')}</span>
          <input name="gecos" autocomplete="off" ?disabled=${this.busy} />
        </label>
        <label>
          <span>${msg('Home (optional)')}</span>
          <input name="home" autocomplete="off" ?disabled=${this.busy} />
        </label>
        <label>
          <span>${msg('Shell (optional)')}</span>
          <input name="shell" autocomplete="off" ?disabled=${this.busy} />
        </label>
        <label class="check">
          <input name="system" type="checkbox" ?disabled=${this.busy || !snapshot.allow_system} />
          <span>${msg('System account')}</span>
        </label>
        <button type="submit" ?disabled=${this.busy}>${msg('Create user')}</button>
      </form>
      <p class="hint">
        ${msg('New accounts are created locked. The daemon validates names, ids, homes and shells.')}
      </p>
    `
  }

  private renderCreateGroup(snapshot: AccountSnapshot) {
    if (!snapshot.control_enabled) {
      return html`<p class="hint">${msg('Account changes are disabled; set accounts.control to enable them.')}</p>`
    }
    return html`
      <form
        class="create compact"
        part="create-group"
        @submit=${(event: Event) => this.submitCreateGroup(event)}
      >
        <label>
          <span>${msg('Name')}</span>
          <input name="name" autocomplete="off" ?disabled=${this.busy} />
        </label>
        <label>
          <span>${msg('GID (optional)')}</span>
          <input name="gid" type="number" min="1" ?disabled=${this.busy} />
        </label>
        <label class="check">
          <input name="system" type="checkbox" ?disabled=${this.busy || !snapshot.allow_system} />
          <span>${msg('System group')}</span>
        </label>
        <button type="submit" ?disabled=${this.busy}>${msg('Create group')}</button>
      </form>
    `
  }

  private renderUser(user: AccountUser, snapshot: AccountSnapshot) {
    const password = canSetPassword(user, snapshot)
    const remove = canDeleteUser(user, snapshot)
    return html`
      <article class="user" data-source=${user.source} part="user">
        <div class="identity">
          <div class="title">
            <span class="dot" aria-hidden="true"></span>
            <h3>${user.name}</h3>
            ${user.system ? this.badge(msg('system'), 'warn') : nothing}
            ${user.source === 'nss' ? this.badge('nss', 'warn') : nothing}
            ${this.badge(
              user.locked === undefined
                ? msg('lock unknown')
                : user.locked
                  ? msg('locked')
                  : msg('unlocked'),
              user.locked === undefined ? 'warn' : '',
            )}
          </div>
          <p class="meta">
            <span>uid ${user.uid}</span>
            <span>gid ${user.gid}</span>
            <span>${msg('primary group')} ${user.group}</span>
            ${user.gecos ? html`<span>${user.gecos}</span>` : nothing}
          </p>
          <p class="meta">
            <span>${user.home || '—'}</span>
            <span>${user.shell || '—'}</span>
          </p>
        </div>
        <div class="members">
          ${user.groups.length === 0
            ? nothing
            : html`<div class="chips">
                ${user.groups.map((group) => html`<span class="chip">${group}</span>`)}
              </div>`}
        </div>
        <div class="actions">
          ${password
            ? html`<button
                type="button"
                ?disabled=${this.busy}
                @click=${() => {
                  this.passwordFor = this.passwordFor === user.name ? undefined : user.name
                  this.confirmingDelete = undefined
                }}
              >
                ${msg('Password')}
              </button>`
            : nothing}
          ${remove
            ? html`<button
                type="button"
                class="danger"
                ?disabled=${this.busy}
                @click=${() => {
                  this.confirmingDelete =
                    this.confirmingDelete === user.name ? undefined : user.name
                  this.passwordFor = undefined
                  this.removeHome = false
                }}
              >
                ${msg('Delete')}
              </button>`
            : nothing}
        </div>
        ${this.passwordFor === user.name
          ? html`
              <form
                class="manage inline"
                @submit=${(event: Event) => this.submitPassword(event, user)}
              >
                <label>
                  <span>${msg('Password')}</span>
                  <input
                    name="password"
                    type="password"
                    autocomplete="new-password"
                    ?disabled=${this.busy}
                  />
                </label>
                <label>
                  <span>${msg('Confirm password')}</span>
                  <input
                    name="confirm"
                    type="password"
                    autocomplete="new-password"
                    ?disabled=${this.busy}
                  />
                </label>
                <button type="submit" ?disabled=${this.busy}>${msg('Set password')}</button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.passwordFor = undefined
                  }}
                >
                  ${msg('Cancel')}
                </button>
              </form>
            `
          : nothing}
        ${this.confirmingDelete === user.name
          ? html`
              <div class="manage confirm" role="alert">
                <span>${msg(str`Delete user ${user.name}?`)}</span>
                <label class="check">
                  <input
                    type="checkbox"
                    .checked=${this.removeHome}
                    ?disabled=${this.busy}
                    @change=${(event: Event) => {
                      this.removeHome = (event.currentTarget as HTMLInputElement).checked
                    }}
                  />
                  <span>${msg('Remove the home directory too')}</span>
                </label>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => this.submitDeleteUser(user)}
                >
                  ${msg('Delete')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.confirmingDelete = undefined
                    this.removeHome = false
                  }}
                >
                  ${msg('Cancel')}
                </button>
              </div>
            `
          : nothing}
      </article>
    `
  }

  private renderGroup(group: AccountGroup, snapshot: AccountSnapshot) {
    const remove = canDeleteGroup(group, snapshot)
    return html`
      <article class="user" data-source=${group.source} part="group">
        <div class="identity">
          <div class="title">
            <span class="dot" aria-hidden="true"></span>
            <h3>${group.name}</h3>
            ${group.system ? this.badge(msg('system'), 'warn') : nothing}
            ${group.source === 'nss' ? this.badge('nss', 'warn') : nothing}
          </div>
          <p class="meta">
            <span>gid ${group.gid}</span>
          </p>
        </div>
        <div class="members">
          ${group.members.length === 0
            ? html`<span class="muted">${msg('no members')}</span>`
            : html`<div class="chips">
                ${group.members.map((member) => html`<span class="chip">${member}</span>`)}
              </div>`}
        </div>
        <div class="actions">
          ${remove
            ? html`<button
                type="button"
                class="danger"
                ?disabled=${this.busy}
                @click=${() => {
                  this.confirmingDelete =
                    this.confirmingDelete === group.name ? undefined : group.name
                }}
              >
                ${msg('Delete')}
              </button>`
            : nothing}
        </div>
        ${this.confirmingDelete === group.name
          ? html`
              <div class="manage confirm" role="alert">
                <span>${msg(str`Delete group ${group.name}?`)}</span>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => this.submitDeleteGroup(group)}
                >
                  ${msg('Delete')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.confirmingDelete = undefined
                  }}
                >
                  ${msg('Cancel')}
                </button>
              </div>
            `
          : nothing}
      </article>
    `
  }

  private renderUsers(snapshot: AccountSnapshot) {
    const filtered = filterUsers(snapshot.users, this.query)
    return html`
      ${this.renderCreateUser(snapshot)}
      ${this.renderFilter(msg('Filter users'))}
      ${filtered.length === 0
        ? html`<p class="empty">
            ${snapshot.users.length === 0
              ? msg('No users found.')
              : msg('No users match the filter.')}
          </p>`
        : html`<div class="list" part="users">
            ${filtered.map((user) => this.renderUser(user, snapshot))}
          </div>`}
    `
  }

  private renderGroups(snapshot: AccountSnapshot) {
    const filtered = filterGroups(snapshot.groups, this.query)
    return html`
      ${this.renderCreateGroup(snapshot)}
      ${this.renderFilter(msg('Filter groups'))}
      ${filtered.length === 0
        ? html`<p class="empty">
            ${snapshot.groups.length === 0
              ? msg('No groups found.')
              : msg('No groups match the filter.')}
          </p>`
        : html`<div class="list" part="groups">
            ${filtered.map((group) => this.renderGroup(group, snapshot))}
          </div>`}
    `
  }

  private renderBody() {
    const snapshot = this.snapshot
    if (!snapshot) {
      if (this.status === 'error') {
        return html`<p class="error" role="alert">${this.error}</p>`
      }
      return html`<p class="empty">${msg('Loading accounts…')}</p>`
    }
    return html`
      ${snapshot.warning
        ? html`<p class="warning" role="status">${snapshot.warning}</p>`
        : nothing}
      ${this.error ? html`<p class="error" role="alert">${this.error}</p>` : nothing}
      ${this.actionError
        ? html`<p class="error" role="alert">${this.actionError}</p>`
        : nothing}
      ${this.notice ? html`<p class="notice" role="status">${this.notice}</p>` : nothing}
      ${this.renderContext(snapshot)}
      ${this.view === 'groups' ? this.renderGroups(snapshot) : this.renderUsers(snapshot)}
    `
  }

  override render() {
    return html`
      ${this.client ? this.renderToolbar() : nothing}
      <div class="body" part="body">${this.renderBody()}</div>
    `
  }

  static styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 14px/1.45 var(--sans, system-ui, sans-serif);
    }

    * { box-sizing: border-box; }

    .body { display: flex; flex-direction: column; gap: 0.75rem; }

    .toolbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.5rem 0.75rem;
      margin-bottom: 0.75rem;
      padding: 0.5rem 0.65rem;
      border-radius: 8px;
      border: 1px solid var(--panel-border, #223041);
      background: var(--panel, #101721);
    }
    .status {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }
    .status::before {
      content: '';
      width: 6px;
      height: 6px;
      border-radius: 999px;
      background: currentColor;
    }
    .status[data-state='ready'] { color: var(--ok, #34d399); }
    .status[data-state='error'] { color: var(--err, #f87171); }
    .status[data-state='loading'] { color: var(--accent, #7dd3fc); }
    .count { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
    .spacer { flex: 1; }
    .toolbar button {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      min-height: 30px;
      padding: 0.25rem 0.55rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .toolbar button:hover { border-color: var(--accent, #7dd3fc); }
    .toolbar button:disabled { opacity: 0.55; cursor: progress; }
    .toolbar anth-icon { font-size: 14px; }

    .context {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.4rem 0.6rem;
      padding: 0.5rem 0.65rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel-2, #0b121b);
      font: 12px/1.4 var(--mono, monospace);
    }
    .context-label {
      color: var(--muted, #8b9bad);
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
    .policies { display: inline-flex; flex-wrap: wrap; gap: 0.35rem; margin-left: auto; }
    .policy {
      padding: 0.1rem 0.4rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font-size: 10px;
      text-transform: uppercase;
    }
    .policy[data-on='yes'] { color: var(--ok, #34d399); }

    .create {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
      align-items: end;
      gap: 0.5rem 0.75rem;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .create label,
    .inline label {
      display: grid;
      gap: 0.2rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }
    .create input,
    .create select,
    .inline input,
    .inline select {
      min-height: 32px;
      width: 100%;
      padding: 0.3rem 0.5rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
    }
    .create .check,
    .confirm .check {
      display: flex;
      grid-auto-flow: column;
      align-items: center;
      align-self: end;
      gap: 0.35rem;
      min-height: 32px;
    }
    .create .check input,
    .confirm .check input { min-height: 0; width: auto; }
    .create button,
    .inline button,
    .actions button,
    .confirm button {
      min-height: 32px;
      padding: 0.3rem 0.6rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .create button:hover,
    .inline button:hover,
    .actions button:hover,
    .confirm button:hover { border-color: var(--accent, #7dd3fc); }
    .create button:disabled,
    .inline button:disabled,
    .actions button:disabled,
    .confirm button:disabled { opacity: 0.55; cursor: progress; }
    .actions button.danger,
    .confirm button.danger { color: var(--err, #f87171); }
    .hint { margin: -0.25rem 0 0; color: var(--muted, #8b9bad); font-size: 11px; }

    .filters { display: flex; }
    .filters input {
      flex: 1 1 16rem;
      min-height: 34px;
      padding: 0.4rem 0.6rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel-2, #0b121b);
      font: inherit;
    }
    .filters input::placeholder { color: var(--muted, #8b9bad); }
    .filters input:focus-visible,
    .toolbar button:focus-visible,
    .create button:focus-visible,
    .actions button:focus-visible,
    .inline button:focus-visible,
    .confirm button:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .list { display: flex; flex-direction: column; gap: 0.5rem; }
    .user {
      display: grid;
      grid-template-columns: minmax(14rem, 1.4fr) minmax(8rem, 1fr) auto;
      align-items: start;
      gap: 0.45rem 0.9rem;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-left: 3px solid var(--accent, #7dd3fc);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .user[data-source='nss'] { border-left-color: var(--muted, #8b9bad); }
    .user .title { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem; }
    .user h3 { margin: 0; font: 600 15px/1.3 var(--mono, monospace); }
    .dot { width: 7px; height: 7px; border-radius: 999px; background: var(--accent, #7dd3fc); }
    .user[data-source='nss'] .dot { background: var(--muted, #8b9bad); }
    .badge {
      padding: 0.1rem 0.35rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }
    .badge[data-tone='warn'] { color: var(--warn, #fbbf24); }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: 0.25rem 0.6rem;
      margin: 0.2rem 0 0;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      overflow-wrap: anywhere;
    }
    .members { min-width: 0; }
    .chips { display: flex; flex-wrap: wrap; gap: 0.25rem 0.4rem; }
    .chip {
      padding: 0.1rem 0.4rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: var(--panel-2, #0b121b);
      color: var(--text, #dbe6f2);
      font: 12px/1.45 var(--mono, monospace);
      overflow-wrap: anywhere;
    }
    .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 0.4rem; }
    .manage {
      grid-column: 1 / -1;
      display: flex;
      flex-wrap: wrap;
      align-items: end;
      gap: 0.45rem;
    }
    .manage.confirm {
      align-items: center;
      padding: 0.4rem 0.5rem;
      border: 1px solid rgba(248, 113, 113, 0.4);
      border-radius: 7px;
      background: rgba(248, 113, 113, 0.06);
      font-size: 12px;
    }
    .manage.confirm > span { margin-right: auto; }

    .muted { color: var(--muted, #8b9bad); }
    .notice {
      margin: 0;
      padding: 0.75rem;
      border: 1px solid rgba(52, 211, 153, 0.35);
      border-radius: 8px;
      background: var(--panel, #101721);
      color: var(--ok, #34d399);
    }
    .empty,
    .error,
    .warning {
      margin: 0;
      padding: 0.75rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
    }
    .empty { color: var(--muted, #8b9bad); }
    .error { color: var(--err, #f87171); }
    .warning { color: var(--warn, #fbbf24); }

    @media (max-width: 44rem) {
      .toolbar button { min-height: 44px; }
      .filters input { min-height: 44px; }
      .create { grid-template-columns: 1fr 1fr; }
      .create button,
      .actions button,
      .inline button,
      .confirm button { min-height: 44px; }
      .create input,
      .create select,
      .inline input,
      .inline select { min-height: 44px; }
      .user { grid-template-columns: minmax(0, 1fr); }
      .actions { justify-content: flex-start; }
      .manage { grid-column: 1; }
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-accounts': AnthAccounts
  }
}
