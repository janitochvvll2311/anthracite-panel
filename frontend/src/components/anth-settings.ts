/**
 * Settings form for the panel configuration.
 *
 * Every field is a control bound to one value of `ConfigView`; the component
 * sends only the sections the operator touched, because a full replacement would
 * overwrite a field changed by another tab in the meantime. The password is not
 * part of this form: `POST /api/auth/password` checks the current password and
 * revokes sessions, which a plain field cannot express.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import '../components/anth-modal'
import type { ConfigPatch, ConfigView } from '../lib/config/config-client'

@customElement('anth-settings')
@localized()
export class AnthSettings extends LitElement {
  static override styles = css`
    :host {
      display: block;
    }

    section {
      margin-bottom: 1.4rem;
      padding: 1rem 1.1rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: rgba(16, 23, 33, 0.72);
    }

    h2 {
      margin: 0 0 0.8rem;
      font: 600 13px/1.3 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--accent, #7dd3fc);
    }

    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 0.8rem 1.2rem;
    }

    label {
      display: grid;
      gap: 0.3rem;
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.05em;
      text-transform: uppercase;
      color: var(--muted, #8b9bad);
    }

    label.check {
      grid-template-columns: auto 1fr;
      align-items: center;
      gap: 0.5rem;
      text-transform: none;
      letter-spacing: 0;
    }

    input[type='text'],
    input[type='number'] {
      padding: 0.45rem 0.55rem;
      color: inherit;
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel, #101721);
    }

    .actions {
      display: flex;
      gap: 0.6rem;
      align-items: center;
    }

    button {
      padding: 0.45rem 0.8rem;
      font: 700 12px/1.2 var(--mono, monospace);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel, #101721);
      color: inherit;
      cursor: pointer;
    }

    button.primary {
      color: #06121c;
      border-color: transparent;
      background: var(--accent, #7dd3fc);
    }

    button:disabled {
      opacity: 0.55;
      cursor: not-allowed;
    }

    .status {
      margin: 0;
      font-size: 13px;
    }

    .status.error {
      color: var(--err, #f87171);
    }

    .status.ok {
      color: var(--ok, #4ade80);
    }
  `

  /** Injected client; the harness wires the HTTP one and the tests a fake. */
  @property({ attribute: false })
  client?: {
    read: () => Promise<ConfigView>
    patch: (patch: ConfigPatch) => Promise<ConfigView>
  }

  @state()
  private config: ConfigView | null = null

  @state()
  private draft: ConfigView | null = null

  @state()
  private status = ''

  @state()
  private failed = false

  @state()
  private saving = false

  override async connectedCallback(): Promise<void> {
    super.connectedCallback()
    await this.reload()
  }

  async reload(): Promise<void> {
    if (!this.client) return
    try {
      const config = await this.client.read()
      this.config = config
      this.draft = structuredClone(config)
      this.status = ''
      this.failed = false
    } catch (error) {
      this.status = error instanceof Error ? error.message : String(error)
      this.failed = true
    }
  }

  override render() {
    if (!this.draft) {
      return html`<p class=${this.failed ? 'status error' : 'status'}>
        ${this.status || msg('Loading…')}
      </p>`
    }

    return html`
      ${this.renderSection(
        msg('Access'),
        html`
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.sudo}
              @change=${(event: Event) =>
                this.setValue('sudo', (event.target as HTMLInputElement).checked)}
            />
            ${msg('Run privileged commands through sudo')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.files.write}
              @change=${(event: Event) =>
                this.setNested('files', 'write', (event.target as HTMLInputElement).checked)}
            />
            ${msg('Allow file writes')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.process.control}
              @change=${(event: Event) =>
                this.setNested('process', 'control', (event.target as HTMLInputElement).checked)}
            />
            ${msg('Allow process signals')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.network.control}
              @change=${(event: Event) =>
                this.setNested('network', 'control', (event.target as HTMLInputElement).checked)}
            />
            ${msg('Allow interface up and down')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.firewall.control}
              @change=${(event: Event) =>
                this.setNested('firewall', 'control', (event.target as HTMLInputElement).checked)}
            />
            ${msg('Allow firewall changes')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.volumes.control}
              @change=${(event: Event) =>
                this.setNested('volumes', 'control', (event.target as HTMLInputElement).checked)}
            />
            ${msg('Allow mount and unmount')}
          </label>
        `,
      )}
      ${this.renderSection(
        msg('Paths'),
        html`
          <label>
            ${msg('Files root')}
            <input
              type="text"
              .value=${this.draft.files.root}
              @change=${(event: Event) =>
                this.setNested('files', 'root', (event.target as HTMLInputElement).value)}
            />
          </label>
          <label>
            ${msg('Mount root')}
            <input
              type="text"
              .value=${this.draft.volumes.mount_root}
              @change=${(event: Event) =>
                this.setNested('volumes', 'mount_root', (event.target as HTMLInputElement).value)}
            />
          </label>
          <label>
            ${msg('Socket path')}
            <input
              type="text"
              .value=${this.draft.socket_path}
              @change=${(event: Event) =>
                this.setValue('socket_path', (event.target as HTMLInputElement).value)}
            />
          </label>
          <label>
            ${msg('Terminal shell')}
            <input
              type="text"
              .value=${this.draft.terminal.shell}
              @change=${(event: Event) =>
                this.setNested('terminal', 'shell', (event.target as HTMLInputElement).value)}
            />
          </label>
        `,
      )}
      ${this.renderSection(
        msg('Limits'),
        html`
          <label>
            ${msg('Max terminal sessions')}
            <input
              type="number"
              min="1"
              .value=${String(this.draft.terminal.max_sessions)}
              @change=${(event: Event) =>
                this.setNested(
                  'terminal',
                  'max_sessions',
                  Number((event.target as HTMLInputElement).value),
                )}
            />
          </label>
          <label>
            ${msg('Max directory entries')}
            <input
              type="number"
              min="1"
              .value=${String(this.draft.files.max_entries)}
              @change=${(event: Event) =>
                this.setNested(
                  'files',
                  'max_entries',
                  Number((event.target as HTMLInputElement).value),
                )}
            />
          </label>
          <label>
            ${msg('Max preview bytes')}
            <input
              type="number"
              min="0"
              .value=${String(this.draft.files.max_preview_bytes)}
              @change=${(event: Event) =>
                this.setNested(
                  'files',
                  'max_preview_bytes',
                  Number((event.target as HTMLInputElement).value),
                )}
            />
          </label>
          <label>
            ${msg('Max upload bytes')}
            <input
              type="number"
              min="0"
              .value=${String(this.draft.files.max_upload_bytes)}
              @change=${(event: Event) =>
                this.setNested(
                  'files',
                  'max_upload_bytes',
                  Number((event.target as HTMLInputElement).value),
                )}
            />
          </label>
        `,
      )}
      ${this.renderSection(
        msg('Accounts'),
        html`
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.accounts.control}
              @change=${(event: Event) =>
                this.setNested('accounts', 'control', (event.target as HTMLInputElement).checked)}
            />
            ${msg('Allow account changes')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.accounts.manage_passwords}
              @change=${(event: Event) =>
                this.setNested(
                  'accounts',
                  'manage_passwords',
                  (event.target as HTMLInputElement).checked,
                )}
            />
            ${msg('Allow password changes')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.accounts.allow_system}
              @change=${(event: Event) =>
                this.setNested(
                  'accounts',
                  'allow_system',
                  (event.target as HTMLInputElement).checked,
                )}
            />
            ${msg('Allow system accounts')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.accounts.allow_privileged_groups}
              @change=${(event: Event) =>
                this.setNested(
                  'accounts',
                  'allow_privileged_groups',
                  (event.target as HTMLInputElement).checked,
                )}
            />
            ${msg('Allow privileged groups')}
          </label>
        `,
      )}
      ${this.renderSection(
        msg('Session'),
        html`
          <label>
            ${msg('Account name')}
            <input
              type="text"
              .value=${this.draft.auth.username}
              @change=${(event: Event) =>
                this.setNested('auth', 'username', (event.target as HTMLInputElement).value)}
            />
          </label>
          <label>
            ${msg('Access token lifetime (seconds)')}
            <input
              type="number"
              min="1"
              .value=${String(this.draft.auth.access_token_secs)}
              @change=${(event: Event) =>
                this.setNested(
                  'auth',
                  'access_token_secs',
                  Number((event.target as HTMLInputElement).value),
                )}
            />
          </label>
          <label>
            ${msg('Refresh token lifetime (seconds)')}
            <input
              type="number"
              min="1"
              .value=${String(this.draft.auth.refresh_token_secs)}
              @change=${(event: Event) =>
                this.setNested(
                  'auth',
                  'refresh_token_secs',
                  Number((event.target as HTMLInputElement).value),
                )}
            />
          </label>
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.draft.auth.cookie_secure}
              @change=${(event: Event) =>
                this.setNested(
                  'auth',
                  'cookie_secure',
                  (event.target as HTMLInputElement).checked,
                )}
            />
            ${msg('Mark the auth cookies Secure (needs HTTPS)')}
          </label>
          <p class="status">
            ${msg('Password set:')}
            ${this.draft.auth.password_set ? msg('yes') : msg('no')}
            ${' · '}
            ${msg('Change it with `anth-panel auth set-password` on the host.')}
          </p>
        `,
      )}
      <div class="actions">
        <button class="primary" ?disabled=${this.saving} @click=${this.save}>
          ${this.saving ? msg('Saving…') : msg('Save')}
        </button>
        <button ?disabled=${this.saving} @click=${this.reload}>
          ${msg('Reload')}
        </button>
        ${this.status
          ? html`<p class=${`status ${this.failed ? 'error' : 'ok'}`} role="status">
              ${this.status}
            </p>`
          : nothing}
      </div>
    `
  }

  private renderSection(title: string, body: unknown) {
    return html`<section>
      <h2>${title}</h2>
      <div class="grid">${body}</div>
    </section>`
  }

  private setValue<K extends keyof ConfigView>(
    key: K,
    value: ConfigView[K],
  ): void {
    if (!this.draft) return
    this.draft = { ...this.draft, [key]: value }
  }

  private setNested<K extends keyof ConfigView>(
    section: K,
    key: keyof ConfigView[K],
    value: unknown,
  ): void {
    if (!this.draft) return
    const current = this.draft[section] as Record<string, unknown>
    this.draft = {
      ...this.draft,
      [section]: { ...current, [key]: value } as ConfigView[K],
    }
  }

  /** Sends the sections that differ from the loaded view. */
  private buildPatch(): ConfigPatch {
    const patch: Record<string, unknown> = {}
    if (!this.config || !this.draft) return patch

    for (const key of Object.keys(this.config) as Array<keyof ConfigView>) {
      const before = this.config[key]
      const after = this.draft[key]
      if (typeof before === 'object' && before !== null) {
        const changed: Record<string, unknown> = {}
        const beforeRecord = before as Record<string, unknown>
        const afterRecord = after as Record<string, unknown>
        for (const field of Object.keys(beforeRecord)) {
          if (beforeRecord[field] !== afterRecord[field]) {
            changed[field] = afterRecord[field]
          }
        }
        if (Object.keys(changed).length > 0) patch[key] = changed
      } else if (before !== after) {
        patch[key] = after
      }
    }
    return patch as ConfigPatch
  }

  private async save(): Promise<void> {
    if (!this.client || !this.draft) return
    const patch = this.buildPatch()
    if (Object.keys(patch).length === 0) {
      this.status = msg('Nothing changed.')
      this.failed = false
      return
    }

    this.saving = true
    try {
      const config = await this.client.patch(patch)
      this.config = config
      this.draft = structuredClone(config)
      this.status = msg('Saved.')
      this.failed = false
    } catch (error) {
      this.status = error instanceof Error ? error.message : String(error)
      this.failed = true
    } finally {
      this.saving = false
    }
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-settings': AnthSettings
  }
}
