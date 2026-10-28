/** `anth-plugins`: the signed-bundle catalogue inside Settings. */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import { describeError } from './errors'
import type { PluginView, PluginsClient, PluginsView } from './plugins-client'

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error'

@customElement('anth-plugins')
@localized()
export class AnthPlugins extends LitElement {
  @property({ attribute: false })
  client?: PluginsClient

  @state()
  private snapshot?: PluginsView

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
  private warning = ''

  @state()
  private trusted = false

  @state()
  private confirmingRemove?: string

  @state()
  private confirmingApprove?: string

  override connectedCallback(): void {
    super.connectedCallback()
    void this.load()
  }

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
      this.warning = ''
    } catch (failure) {
      this.error = describeError(failure)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private async run(
    task: (client: PluginsClient) => Promise<unknown>,
    done?: () => void,
  ): Promise<void> {
    const client = this.client
    if (!client || this.busy) return
    this.busy = true
    this.actionError = ''
    this.notice = ''
    this.warning = ''
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

  private async install(event: Event): Promise<void> {
    event.preventDefault()
    if (this.busy || !this.client) return
    const form = event.currentTarget as HTMLFormElement
    const input = form.querySelector<HTMLInputElement>('input[type="file"]')
    const file = input?.files?.[0]
    if (!file) {
      this.actionError = msg('Choose a signed .zip bundle.')
      return
    }
    if (!this.trusted) {
      this.actionError = msg('Confirm that you trust this bundle before installing it.')
      return
    }
    this.busy = true
    this.actionError = ''
    this.notice = ''
    this.warning = ''
    try {
      const encoded = toBase64(await file.arrayBuffer())
      const result = await this.client.install(encoded, false)
      this.snapshot = await this.client.get()
      this.status = 'ready'
      this.notice = result.warning
        ? msg(str`Installed ${result.id}, but ${result.warning}`)
        : msg(str`Installed ${result.id} ${result.version}.`)
      this.warning = result.warning ?? ''
      this.trusted = false
      form.reset()
    } catch (failure) {
      this.actionError = describeError(failure)
    } finally {
      this.busy = false
    }
  }

  private countLabel(): string {
    if (!this.snapshot) return ''
    const count = this.snapshot.plugins.length
    const enabled = this.snapshot.plugins.filter((plugin) => plugin.enabled).length
    return count === 1
      ? msg(str`1 plugin · active: ${enabled}`)
      : msg(str`${count} plugins · active: ${enabled}`)
  }

  private renderToolbar() {
    return html`
      <div class="toolbar" part="toolbar">
        <span class="status" data-state=${this.status} role="status">
          ${this.status === 'loading'
            ? msg('loading')
            : this.status === 'error'
              ? msg('error')
              : this.snapshot
                ? msg('ready')
                : msg('idle')}
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

  private renderContext(snapshot: PluginsView) {
    return html`
      <div class="context" part="context">
        <span class="context-label">${msg('Policy')}</span>
        <span class="policies">
          <span class="policy" data-on=${snapshot.enabled ? 'yes' : 'no'}>
            ${msg('plugins')}
          </span>
          <span class="policy" data-on=${snapshot.install_from_web ? 'yes' : 'no'}>
            ${msg('web install')}
          </span>
        </span>
      </div>
      ${snapshot.enabled
        ? nothing
        : html`<p class="warning" role="status">
            ${msg('Plugins are disabled; set [plugins] enabled = true to load them.')}
          </p>`}
    `
  }

  private renderInstall(snapshot: PluginsView) {
    if (!snapshot.enabled) return nothing
    if (!snapshot.install_from_web) {
      return html`<p class="hint">
        ${msg('Browser installs are off. Install a bundle with anth-panel plugin install.')}
      </p>`
    }
    return html`
      <form class="install" part="install" @submit=${(event: Event) => void this.install(event)}>
        <label>
          <span>${msg('Signed bundle (.zip)')}</span>
          <input
            type="file"
            accept=".zip,application/zip"
            ?disabled=${this.busy}
            @change=${() => {
              this.actionError = ''
            }}
          />
        </label>
        <label class="check">
          <input
            type="checkbox"
            .checked=${this.trusted}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.trusted = (event.currentTarget as HTMLInputElement).checked
            }}
          />
          <span>${msg('I trust this bundle; it can run code as the daemon.')}</span>
        </label>
        <button type="submit" ?disabled=${this.busy || !this.trusted}>
          ${msg('Install')}
        </button>
      </form>
    `
  }

  private renderFilter() {
    return html`
      <div class="filters" part="filters">
        <input
          type="search"
          .value=${this.query}
          placeholder=${msg('Filter plugins')}
          aria-label=${msg('Filter plugins')}
          @input=${(event: Event) => {
            this.query = (event.currentTarget as HTMLInputElement).value
          }}
        />
      </div>
    `
  }

  private stateLabel(plugin: PluginView): string {
    if (plugin.state === 'enabled') return msg('enabled')
    if (plugin.state === 'disabled') return msg('disabled')
    if (plugin.state === 'unapproved') return msg('unapproved')
    return msg('error')
  }

  private renderPlugin(plugin: PluginView) {
    return html`
      <article class="plugin" data-state=${plugin.state} part="plugin">
        <div class="identity">
          <div class="title">
            <span class="dot" aria-hidden="true"></span>
            <h3>${plugin.name}</h3>
            <span class="badge" data-tone=${plugin.state === 'enabled' ? 'ok' : 'warn'}>
              ${this.stateLabel(plugin)}
            </span>
            ${plugin.alias ? html`<span class="badge">${plugin.alias}</span>` : nothing}
          </div>
          <p class="meta">
            <code>${plugin.id}</code>
            ${plugin.version ? html`<span>${plugin.version}</span>` : nothing}
          </p>
          ${plugin.error
            ? html`<p class="reason" role="status">${plugin.error}</p>`
            : nothing}
        </div>
        <div class="surface">
          ${plugin.capabilities.length === 0
            ? nothing
            : html`<div class="chips">
                ${plugin.capabilities.map(
                  (capability) => html`<span class="chip">${capability}</span>`,
                )}
              </div>`}
          ${plugin.api.length === 0
            ? nothing
            : html`<p class="meta">
                ${plugin.api.map(
                  (anchor) => html`<span class="chip">${anchor.method} ${anchor.path}</span>`,
                )}
              </p>`}
        </div>
        <div class="actions">
          ${plugin.state === 'enabled'
            ? html`<button
                type="button"
                ?disabled=${this.busy}
                @click=${() =>
                  void this.run((client) => client.disable(plugin.id))}
              >
                ${msg('Disable')}
              </button>`
            : plugin.state === 'disabled'
              ? html`<button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => void this.run((client) => client.enable(plugin.id))}
                >
                  ${msg('Enable')}
                </button>`
              : nothing}
          ${plugin.state === 'error' || plugin.state === 'unapproved'
            ? html`<button
                type="button"
                ?disabled=${this.busy}
                @click=${() => {
                  this.confirmingApprove =
                    this.confirmingApprove === plugin.id ? undefined : plugin.id
                  this.confirmingRemove = undefined
                }}
              >
                ${msg('Approve')}
              </button>`
            : nothing}
          <button
            type="button"
            class="danger"
            ?disabled=${this.busy}
            @click=${() => {
              this.confirmingRemove =
                this.confirmingRemove === plugin.id ? undefined : plugin.id
              this.confirmingApprove = undefined
            }}
          >
            ${msg('Remove')}
          </button>
        </div>
        ${this.confirmingApprove === plugin.id
          ? html`
              <div class="manage confirm" role="alert">
                <span>${msg(str`Approve the capabilities of ${plugin.id}?`)}</span>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() =>
                    void this.run(
                      (client) => client.approve(plugin.id, true),
                      () => {
                        this.confirmingApprove = undefined
                      },
                    )}
                >
                  ${msg('Approve')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.confirmingApprove = undefined
                  }}
                >
                  ${msg('Cancel')}
                </button>
              </div>
            `
          : nothing}
        ${this.confirmingRemove === plugin.id
          ? html`
              <div class="manage confirm" role="alert">
                <span>${msg(str`Remove ${plugin.id} and its grant?`)}</span>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() =>
                    void this.run(
                      (client) => client.remove(plugin.id),
                      () => {
                        this.confirmingRemove = undefined
                      },
                    )}
                >
                  ${msg('Remove')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.confirmingRemove = undefined
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

  private filtered(snapshot: PluginsView): PluginView[] {
    const needle = this.query.trim().toLowerCase()
    if (needle === '') return snapshot.plugins
    return snapshot.plugins.filter((plugin) =>
      [
        plugin.id,
        plugin.alias ?? '',
        plugin.name,
        plugin.version,
        ...plugin.capabilities,
        ...plugin.api.map((anchor) => `${anchor.method} ${anchor.path}`),
      ].some((value) => value.toLowerCase().includes(needle)),
    )
  }

  private renderBody() {
    const snapshot = this.snapshot
    if (!snapshot) {
      if (this.status === 'error') {
        return html`<p class="error" role="alert">${this.error}</p>`
      }
      return html`<p class="empty">${msg('Loading plugins…')}</p>`
    }
    const plugins = this.filtered(snapshot)
    return html`
      ${this.error ? html`<p class="error" role="alert">${this.error}</p>` : nothing}
      ${this.actionError
        ? html`<p class="error" role="alert">${this.actionError}</p>`
        : nothing}
      ${this.notice ? html`<p class="notice" role="status">${this.notice}</p>` : nothing}
      ${this.warning ? html`<p class="warning" role="status">${this.warning}</p>` : nothing}
      ${this.renderContext(snapshot)}
      ${this.renderInstall(snapshot)}
      ${this.renderFilter()}
      ${plugins.length === 0
        ? html`<p class="empty">
            ${snapshot.plugins.length === 0
              ? msg('No plugins installed.')
              : msg('No plugins match the filter.')}
          </p>`
        : html`<div class="list" part="plugins">
            ${plugins.map((plugin) => this.renderPlugin(plugin))}
          </div>`}
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
    .toolbar button,
    .install button,
    .actions button,
    .confirm button {
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
    .toolbar button:hover,
    .install button:hover,
    .actions button:hover,
    .confirm button:hover { border-color: var(--accent, #7dd3fc); }
    .toolbar button:disabled,
    .install button:disabled,
    .actions button:disabled,
    .confirm button:disabled { opacity: 0.55; cursor: progress; }
    .actions button.danger,
    .confirm button.danger { color: var(--err, #f87171); }
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

    .install {
      display: grid;
      grid-template-columns: minmax(12rem, 1fr) minmax(12rem, 1.4fr) auto;
      align-items: end;
      gap: 0.5rem 0.75rem;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .install label {
      display: grid;
      gap: 0.2rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }
    .install input[type='file'] {
      min-height: 32px;
      padding: 0.3rem 0.5rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
    }
    .install .check,
    .confirm .check {
      display: flex;
      grid-auto-flow: column;
      align-items: center;
      align-self: end;
      gap: 0.35rem;
      min-height: 32px;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--sans, system-ui, sans-serif);
    }
    .install .check input { width: auto; }
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
    .install button:focus-visible,
    .actions button:focus-visible,
    .confirm button:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .list { display: flex; flex-direction: column; gap: 0.5rem; }
    .plugin {
      display: grid;
      grid-template-columns: minmax(14rem, 1.2fr) minmax(12rem, 1fr) auto;
      align-items: start;
      gap: 0.45rem 0.9rem;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-left: 3px solid var(--muted, #8b9bad);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .plugin[data-state='enabled'] { border-left-color: var(--ok, #34d399); }
    .plugin[data-state='error'],
    .plugin[data-state='unapproved'] { border-left-color: var(--err, #f87171); }
    .plugin[data-state='disabled'] { border-left-color: var(--warn, #fbbf24); }
    .title { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem; }
    .plugin h3 { margin: 0; font: 600 15px/1.3 var(--mono, monospace); }
    .dot { width: 7px; height: 7px; border-radius: 999px; background: currentColor; }
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
    .badge[data-tone='ok'] { color: var(--ok, #34d399); }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: 0.25rem 0.6rem;
      margin: 0.2rem 0 0;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      overflow-wrap: anywhere;
    }
    .reason {
      margin: 0.35rem 0 0;
      padding: 0.3rem 0.45rem;
      border-radius: 6px;
      background: rgba(248, 113, 113, 0.08);
      color: var(--err, #f87171);
      font-size: 12px;
    }
    .surface { min-width: 0; }
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
      align-items: center;
      gap: 0.45rem;
    }
    .manage.confirm {
      padding: 0.4rem 0.5rem;
      border: 1px solid rgba(248, 113, 113, 0.4);
      border-radius: 7px;
      background: rgba(248, 113, 113, 0.06);
      font-size: 12px;
    }
    .manage.confirm > span { margin-right: auto; }

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
      .toolbar button,
      .install button,
      .actions button,
      .confirm button { min-height: 44px; }
      .filters input { min-height: 44px; }
      .install { grid-template-columns: 1fr; }
      .install input[type='file'] { min-height: 44px; }
      .plugin { grid-template-columns: minmax(0, 1fr); }
      .actions { justify-content: flex-start; }
      .manage { grid-column: 1; }
    }
  `
}

/** Browser base64 for a bundle; chunks avoid the argument limit of spread. */
function toBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer)
  let binary = ''
  const chunk = 0x8000
  for (let offset = 0; offset < bytes.length; offset += chunk) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunk))
  }
  return btoa(binary)
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-plugins': AnthPlugins
  }
}
