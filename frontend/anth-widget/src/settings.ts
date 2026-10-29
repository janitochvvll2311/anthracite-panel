import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import type { SessionSummary, Settings, SettingsClient } from 'anth-wire'
import { describeError } from 'anth-wire'
import './fab'
import './icon'

interface Toggle {
  key: string
  label: string
  value: boolean
}

@customElement('anth-settings')
@localized()
export class AnthSettings extends LitElement {
  @property({ attribute: false })
  client?: SettingsClient

  @state()
  private settings?: Settings

  @state()
  private sessions: SessionSummary[] = []

  @state()
  private error = ''

  @state()
  private notice = ''

  @state()
  private busy = false

  @state()
  private dirty = false

  @state()
  private rebootDelay = 0

  override connectedCallback(): void {
    super.connectedCallback()
    queueMicrotask(() => void this.load())
  }

  private async load(): Promise<void> {
    if (!this.client) {
      this.error = msg('No settings client was configured')
      return
    }
    this.busy = true
    try {
      const [settings, sessions] = await Promise.all([
        this.client.get(),
        this.client.sessions(),
      ])
      this.settings = settings
      this.sessions = sessions.sessions
      this.error = ''
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private async perform(action: () => Promise<void>): Promise<void> {
    this.busy = true
    try {
      await action()
      this.error = ''
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private async toggle(key: string, event: Event): Promise<void> {
    const input = event.currentTarget as HTMLInputElement
    await this.perform(async () => {
      const answer = await this.client!.update(key, input.checked)
      this.dirty = this.dirty || answer.restart_required
      this.settings = await this.client!.get()
    })
  }

  private async revokeOthers(): Promise<void> {
    await this.perform(async () => {
      const revoked = await this.client!.revokeOthers()
      this.sessions = (await this.client!.sessions()).sessions
      this.notice = revoked > 0 ? msg('Other sessions were signed out.') : msg('No other sessions.')
    })
  }

  private async restart(): Promise<void> {
    await this.perform(async () => {
      await this.client!.restart()
      this.notice = msg('The panel is restarting…')
      this.dirty = false
    })
  }

  private async reboot(): Promise<void> {
    await this.perform(async () => {
      await this.client!.reboot(this.rebootDelay)
      this.notice = msg('The host is rebooting.')
    })
  }

  private toggles(settings: Settings): Toggle[] {
    return [
      {
        key: 'process.control',
        label: msg('Process control'),
        value: settings.process.control,
      },
      {
        key: 'network.control',
        label: msg('Network control'),
        value: settings.network.control,
      },
      {
        key: 'firewall.control',
        label: msg('Firewall control'),
        value: settings.firewall.control,
      },
      {
        key: 'volumes.control',
        label: msg('Volume control'),
        value: settings.volumes.control,
      },
      {
        key: 'accounts.control',
        label: msg('Account control'),
        value: settings.accounts.control,
      },
      { key: 'files.write', label: msg('File writes'), value: settings.files.write },
      {
        key: 'terminal.enabled',
        label: msg('Terminal'),
        value: settings.terminal.enabled,
      },
      { key: 'apps.enabled', label: msg('Apps'), value: settings.apps.enabled },
    ]
  }

  private renderSessions() {
    if (this.sessions.length === 0) {
      return html`<p class="muted">${msg('No live sessions.')}</p>`
    }
    return html`
      <ul class="sessions">
        ${this.sessions.map(
          (session) => html`
            <li>
              <code>${session.sid.slice(0, 12)}</code>
              <span class="muted">${msg('expires')} ${new Date(session.expires * 1000).toLocaleString()}</span>
              ${session.current
                ? html`<span class="pill">${msg('Current')}</span>`
                : nothing}
            </li>
          `,
        )}
      </ul>
      <button type="button" aria-label=${msg('Sign out other sessions')} ?disabled=${this.busy} @click=${this.revokeOthers}>
        <anth-icon name="logout"></anth-icon>
        <span class="btn-label">${msg('Sign out other sessions')}</span>
      </button>
    `
  }

  private renderToggle(toggle: Toggle) {
    return html`
      <label class="toggle">
        <input
          type="checkbox"
          .checked=${toggle.value}
          ?disabled=${this.busy}
          @change=${(event: Event) => this.toggle(toggle.key, event)}
        />
        <span>${toggle.label}</span>
      </label>
    `
  }

  private renderAccount(settings: Settings) {
    return html`
      <article class="card">
        <h3>${msg('Account')}</h3>
        <dl class="facts">
          <div><dt>${msg('Username')}</dt><dd>${settings.auth.username}</dd></div>
          <div><dt>${msg('Authentication')}</dt><dd>${settings.auth.enabled ? msg('on') : msg('off')}</dd></div>
          <div><dt>${msg('Bootstrap')}</dt><dd>${settings.auth.bootstrap}</dd></div>
        </dl>
      </article>
    `
  }

  private renderSessionsPanel() {
    return html`
      <article class="card">
        <h3>${msg('Sessions')}</h3>
        ${this.renderSessions()}
      </article>
    `
  }

  private renderPolicy(settings: Settings) {
    return html`
      <article class="card">
        <h3>${msg('Policy')}</h3>
        <div class="toggles">
          ${this.toggles(settings).map((toggle) => this.renderToggle(toggle))}
        </div>
      </article>
    `
  }

  private renderPaths(settings: Settings) {
    return html`
      <article class="card">
        <h3>${msg('Paths')}</h3>
        <dl class="facts">
          <div><dt>${msg('Socket')}</dt><dd><code>${settings.socket_path}</code></dd></div>
          <div><dt>${msg('State directory')}</dt><dd><code>${settings.state_dir}</code></dd></div>
          <div><dt>${msg('HTTP address')}</dt><dd><code>${settings.server.addr}</code></dd></div>
          <div><dt>${msg('Log level')}</dt><dd><code>${settings.log.level}</code></dd></div>
        </dl>
      </article>
    `
  }

  private renderLifecycle() {
    return html`
      <article class="card">
        <h3>${msg('Lifecycle')}</h3>
        <p class="muted">${msg('A restart re-reads the configuration. A reboot is refused unless the daemon was started with --allow-reboot.')}</p>
        <div class="actions">
          <button type="button" aria-label=${msg('Restart panel')} ?disabled=${this.busy} @click=${this.restart}>
            <anth-icon name="refresh"></anth-icon>
            <span class="btn-label">${msg('Restart panel')}</span>
          </button>
          <label class="delay">
            ${msg('Delay (seconds)')}
            <input
              type="number"
              min="0"
              max="300"
              .value=${String(this.rebootDelay)}
              @change=${(event: Event) => {
                this.rebootDelay = Number((event.currentTarget as HTMLInputElement).value)
              }}
            />
          </label>
          <button class="danger" type="button" aria-label=${msg('Reboot host')} ?disabled=${this.busy} @click=${this.reboot}>
            <anth-icon name="power"></anth-icon>
            <span class="btn-label">${msg('Reboot host')}</span>
          </button>
        </div>
      </article>
    `
  }

  override render() {
    const settings = this.settings
    return html`
      <section class="settings">
        <header class="bar">
          <div>
            <p class="eyebrow">${msg('Settings')}</p>
            <h2>${msg('General')}</h2>
          </div>
        </header>

        ${this.error ? html`<div class="error" role="alert">${this.error}</div>` : nothing}
        ${this.notice ? html`<div class="notice" role="status">${this.notice}</div>` : nothing}
        ${this.dirty
          ? html`<div class="banner">
              <span>${msg('Restart the panel to apply the changes.')}</span>
              <button type="button" aria-label=${msg('Restart now')} ?disabled=${this.busy} @click=${this.restart}>
                <anth-icon name="refresh"></anth-icon>
                <span class="btn-label">${msg('Restart now')}</span>
              </button>
            </div>`
          : nothing}

        ${!settings
          ? html`<p class="muted">${msg('Loading settings…')}</p>`
          : html`
              ${this.renderAccount(settings)} ${this.renderSessionsPanel()}
              ${this.renderPolicy(settings)} ${this.renderPaths(settings)}
              ${this.renderLifecycle()}
            `}
        <anth-fab
          icon="refresh"
          label=${msg('Reload settings')}
          @click=${() => void this.load()}
        ></anth-fab>
      </section>
    `
  }

  static styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 15px/1.55 var(--sans, system-ui, sans-serif);
    }
    .settings { display: grid; gap: 12px; }
    .bar {
      display: flex;
      align-items: end;
      justify-content: space-between;
      gap: 12px;
    }
    h2 { margin: 0; font: 600 clamp(22px, 3vw, 30px) / 1.1 var(--mono, monospace); }
    h3 {
      margin: 0 0 8px;
      color: var(--muted, #8b9bad);
      font: 600 12px/1.4 var(--mono, monospace);
      letter-spacing: .06em;
      text-transform: uppercase;
    }
    .eyebrow {
      margin: 0;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      letter-spacing: .08em;
      text-transform: uppercase;
    }
    .card {
      padding: 14px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
    }
    .muted { color: var(--muted, #8b9bad); }
    .error { padding: 10px 12px; border-radius: 7px; background: rgba(248, 113, 113, .12); color: #fecaca; }
    .notice { padding: 10px 12px; border-radius: 7px; background: rgba(52, 211, 153, .12); color: #a7f3d0; }
    .banner {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      padding: 10px 12px;
      border-radius: 7px;
      background: rgba(251, 191, 36, .12);
      color: #fde68a;
    }
    .facts { display: grid; gap: 8px; margin: 0; }
    .facts div { display: flex; flex-wrap: wrap; gap: 8px; align-items: baseline; }
    dt { color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); text-transform: uppercase; }
    dd { margin: 0; font: 13px/1.4 var(--mono, monospace); }
    .toggles { display: grid; grid-template-columns: repeat(auto-fit, minmax(190px, 1fr)); gap: 8px; }
    .toggle { display: flex; align-items: center; gap: 8px; }
    .sessions { display: grid; gap: 6px; margin: 0 0 10px; padding: 0; list-style: none; }
    .sessions li { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; font: 13px/1.4 var(--mono, monospace); }
    .pill {
      padding: 1px 7px;
      border-radius: 999px;
      background: rgba(52, 211, 153, .16);
      color: #a7f3d0;
      font-size: 11px;
    }
    .actions { display: flex; flex-wrap: wrap; align-items: end; gap: 10px; }
    .delay { display: grid; gap: 4px; color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
    input, button {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      padding: 7px 10px;
      font: inherit;
    }
    input[type='checkbox'] { accent-color: #34d399; }
    button { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
    anth-icon { font-size: 14px; }
    button:disabled { opacity: .45; cursor: default; }
    button.danger { border-color: rgba(248, 113, 113, .5); color: #fecaca; }
    input:focus-visible, button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    code { font: 12px/1.4 var(--mono, monospace); }
    @media (max-width: 48rem) {
      .btn-label { display: none; }
      button:has(.btn-label) {
        width: 44px;
        height: 44px;
        padding: 0;
        justify-content: center;
      }
    }
  `
}
