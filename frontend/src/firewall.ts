/**
 * `anth-firewall`: the managed nftables table.
 *
 * Unlike the network inventory this panel does not stream: it reads on connect
 * and after every mutation. The daemon owns exactly `inet anthracite` /
 * `input`; the panel never offers another table or chain.
 *
 * Allowing a port removes a matching managed deny rule. It does not add an
 * accept rule, because an accept in this table cannot override another table.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import { describeError } from './errors'
import type {
  FirewallAction,
  FirewallClient,
  FirewallProtocol,
  FirewallRule,
  FirewallRuleAction,
  FirewallSnapshot,
} from './firewall-client'

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error'

@customElement('anth-firewall')
@localized()
export class AnthFirewall extends LitElement {
  @property({ attribute: false })
  client?: FirewallClient

  @state()
  private snapshot?: FirewallSnapshot

  @state()
  private status: LoadStatus = 'idle'

  @state()
  private loading = false

  @state()
  private busy = false

  @state()
  private error = ''

  @state()
  private notice = ''

  @state()
  private action: FirewallAction = 'deny'

  @state()
  private protocol: FirewallProtocol = 'tcp'

  @state()
  private port = ''

  @state()
  private comment = ''

  @state()
  private confirmingFlush = false

  @state()
  private confirmingDelete?: number

  override connectedCallback(): void {
    super.connectedCallback()
    void this.load()
  }

  /** Re-reads the table; the refresh button and every mutation call it. */
  async load(): Promise<void> {
    const client = this.client
    if (!client || this.loading || this.busy) return
    this.loading = true
    if (!this.snapshot) this.status = 'loading'
    try {
      this.snapshot = await client.get()
      this.status = 'ready'
      this.error = ''
    } catch (failure) {
      this.error = describeError(failure)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private async run(
    task: (client: FirewallClient) => Promise<unknown>,
    done?: () => void,
  ): Promise<void> {
    const client = this.client
    if (!client || this.busy) return
    this.busy = true
    this.error = ''
    this.notice = ''
    try {
      await task(client)
      done?.()
      this.snapshot = await client.get()
      this.status = 'ready'
      this.notice = msg('Change applied.')
    } catch (failure) {
      this.error = describeError(failure)
      this.status = 'error'
    } finally {
      this.busy = false
    }
  }

  private submitAdd(event: Event): void {
    event.preventDefault()
    const client = this.client
    if (!client || this.busy) return
    const port = Number(this.port)
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      this.error = msg('The port must be between 1 and 65535.')
      return
    }
    const comment = this.comment.trim()
    void this.run(
      (current) => current.add(this.action, this.protocol, port, comment || undefined),
      () => {
        this.port = ''
        this.comment = ''
      },
    )
  }

  private deleteRule(rule: FirewallRule): void {
    void this.run(
      (client) => client.remove(rule.handle),
      () => {
        this.confirmingDelete = undefined
      },
    )
  }

  private flush(): void {
    void this.run(
      (client) => client.flush(),
      () => {
        this.confirmingFlush = false
      },
    )
  }

  private controlEnabled(): boolean {
    return this.snapshot?.control_enabled === true
  }

  private statusLabel(): string {
    if (this.loading) return msg('loading')
    if (this.status === 'error') return msg('error')
    if (!this.snapshot) return msg('idle')
    return this.snapshot.initialized ? msg('managed') : msg('not initialized')
  }

  private ruleCount(): string {
    if (!this.snapshot?.initialized) return msg('No rules')
    const count = this.snapshot.rules.length
    if (count === 0) return msg('No rules')
    if (count === 1) return msg('1 rule')
    return msg(str`${count} rules`)
  }

  private renderToolbar() {
    return html`
      <div class="toolbar" part="toolbar">
        <span class="status" data-state=${this.status} role="status">
          ${this.statusLabel()}
        </span>
        <span class="count">${this.ruleCount()}</span>
        <span class="spacer"></span>
        <button
          type="button"
          ?disabled=${this.loading || this.busy}
          title=${msg('Refresh')}
          @click=${() => void this.load()}
        >
          <anth-icon name="refresh"></anth-icon>
          <span class="btn-label">${msg('Refresh')}</span>
        </button>
        ${this.controlEnabled()
          ? html`
              <button
                type="button"
                class="danger"
                ?disabled=${this.busy || !this.snapshot?.initialized}
                title=${msg('Flush the managed chain')}
                @click=${() => {
                  this.confirmingFlush = !this.confirmingFlush
                }}
              >
                <anth-icon name="trash"></anth-icon>
                <span class="btn-label">${msg('Flush')}</span>
              </button>
            `
          : nothing}
      </div>
      ${this.confirmingFlush
        ? html`
            <div class="confirm" role="alert">
              <span>${msg('Flush every rule from the managed chain?')}</span>
              <button
                type="button"
                class="danger"
                ?disabled=${this.busy}
                @click=${() => this.flush()}
              >
                ${msg('Flush')}
              </button>
              <button
                type="button"
                ?disabled=${this.busy}
                @click=${() => {
                  this.confirmingFlush = false
                }}
              >
                ${msg('Cancel')}
              </button>
            </div>
          `
        : nothing}
    `
  }

  private renderContext() {
    return html`
      <div class="context" part="context">
        <span class="context-label">${msg('Managed table')}</span>
        <span class="table-name">inet anthracite</span>
        <span class="muted">chain input</span>
        <span class="policy" data-on=${this.controlEnabled() ? 'yes' : 'no'}>
          ${this.controlEnabled() ? msg('control') : msg('read-only')}
        </span>
      </div>
    `
  }

  private renderAddForm() {
    return html`
      <form class="add" part="add" @submit=${(event: Event) => this.submitAdd(event)}>
        <label>
          <span>${msg('Action')}</span>
          <select
            .value=${this.action}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.action = (event.currentTarget as HTMLSelectElement)
                .value as FirewallAction
            }}
          >
            <option value="deny">${msg('Deny')}</option>
            <option value="allow">${msg('Allow')}</option>
          </select>
        </label>
        <label>
          <span>${msg('Port')}</span>
          <input
            type="number"
            min="1"
            max="65535"
            required
            .value=${this.port}
            ?disabled=${this.busy}
            @input=${(event: Event) => {
              this.port = (event.currentTarget as HTMLInputElement).value
            }}
          />
        </label>
        <label>
          <span>${msg('Protocol')}</span>
          <select
            .value=${this.protocol}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.protocol = (event.currentTarget as HTMLSelectElement)
                .value as FirewallProtocol
            }}
          >
            <option value="tcp">tcp</option>
            <option value="udp">udp</option>
          </select>
        </label>
        <label class="grow">
          <span>${msg('Comment (optional)')}</span>
          <input
            maxlength="32"
            .value=${this.comment}
            ?disabled=${this.busy}
            @input=${(event: Event) => {
              this.comment = (event.currentTarget as HTMLInputElement).value
            }}
          />
        </label>
        <button type="submit" ?disabled=${this.busy}>${msg('Add rule')}</button>
      </form>
      <p class="hint">
        ${msg('Allow removes a matching managed deny rule; it does not add an accept rule.')}
      </p>
    `
  }

  private actionLabel(action: FirewallRuleAction): string {
    switch (action) {
      case 'allow':
        return msg('allow')
      case 'deny':
        return msg('deny')
      default:
        return msg('custom')
    }
  }

  private ruleTarget(rule: FirewallRule): string {
    if (rule.protocol && rule.port) return `${rule.protocol}/${rule.port}`
    return '—'
  }

  private renderRule(rule: FirewallRule) {
    return html`
      <article class="rule" part="rule" data-action=${rule.action}>
        <span class="handle">#${rule.handle}</span>
        <span class="badge">${this.actionLabel(rule.action)}</span>
        <span class="target">${this.ruleTarget(rule)}</span>
        <span class="comment">${rule.comment || '—'}</span>
        ${this.controlEnabled()
          ? html`
              <button
                type="button"
                class="danger delete"
                ?disabled=${this.busy}
                title=${msg(str`Delete rule #${rule.handle}`)}
                @click=${() => {
                  this.confirmingDelete =
                    this.confirmingDelete === rule.handle ? undefined : rule.handle
                }}
              >
                ${msg('Delete')}
              </button>
            `
          : nothing}
        ${this.confirmingDelete === rule.handle
          ? html`
              <div class="confirm" role="alert">
                <span>${msg(str`Delete rule #${rule.handle}?`)}</span>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => this.deleteRule(rule)}
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

  private renderBody() {
    if (!this.snapshot) {
      if (this.error) return html`<p class="error" role="alert">${this.error}</p>`
      if (this.client) return html`<p class="empty">${msg('Reading the firewall…')}</p>`
      return html`<p class="empty">${msg('No firewall information yet.')}</p>`
    }

    const rules = this.snapshot.rules
    return html`
      ${this.error ? html`<p class="error" role="alert">${this.error}</p>` : nothing}
      ${this.notice ? html`<p class="notice" role="status">${this.notice}</p>` : nothing}
      ${this.renderContext()}
      ${this.controlEnabled() ? this.renderAddForm() : nothing}
      ${!this.snapshot.initialized
        ? html`
            <p class="empty">
              ${msg('The managed table does not exist yet. Denying a port creates it.')}
            </p>
          `
        : rules.length === 0
          ? html`<p class="empty">${msg('No managed rules. Denying a port adds one.')}</p>`
          : html`<div class="list" part="list">${rules.map((rule) => this.renderRule(rule))}</div>`}
      ${this.snapshot.ruleset
        ? html`
            <details class="raw" part="raw">
              <summary>${msg('Raw ruleset')}</summary>
              <pre>${this.snapshot.ruleset}</pre>
            </details>
          `
        : nothing}
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
    .confirm button:hover { border-color: var(--accent, #7dd3fc); }
    .toolbar button:disabled,
    .confirm button:disabled { opacity: 0.55; cursor: progress; }
    .toolbar button.danger,
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
    .table-name { color: var(--text, #dbe6f2); }
    .policy {
      margin-left: auto;
      padding: 0.1rem 0.4rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font-size: 10px;
      text-transform: uppercase;
    }
    .policy[data-on='yes'] { color: var(--ok, #34d399); }

    .add {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr));
      align-items: end;
      gap: 0.5rem 0.75rem;
      padding: 0.65rem 0.75rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .add label {
      display: grid;
      gap: 0.2rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }
    .add .grow { grid-column: span 2; }
    .add input,
    .add select {
      min-height: 32px;
      padding: 0.3rem 0.5rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
    }
    .add button {
      min-height: 32px;
      padding: 0.3rem 0.65rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .add button:hover { border-color: var(--accent, #7dd3fc); }
    .add button:disabled { opacity: 0.55; cursor: progress; }
    .add input:focus-visible,
    .add select:focus-visible,
    .add button:focus-visible,
    .toolbar button:focus-visible,
    .confirm button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    .hint { margin: -0.25rem 0 0; color: var(--muted, #8b9bad); font-size: 11px; }

    .list { display: flex; flex-direction: column; gap: 0.5rem; }
    .rule {
      display: grid;
      grid-template-columns: auto auto minmax(5rem, auto) 1fr auto;
      align-items: center;
      gap: 0.4rem 0.75rem;
      padding: 0.55rem 0.65rem;
      border: 1px solid var(--panel-border, #223041);
      border-left: 3px solid var(--muted, #8b9bad);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .rule[data-action='deny'] { border-left-color: var(--err, #f87171); }
    .rule[data-action='allow'] { border-left-color: var(--ok, #34d399); }
    .handle { color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); }
    .badge {
      padding: 0.1rem 0.4rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }
    .rule[data-action='deny'] .badge { color: var(--err, #f87171); }
    .rule[data-action='allow'] .badge { color: var(--ok, #34d399); }
    .target { font: 600 13px/1.4 var(--mono, monospace); }
    .comment { color: var(--muted, #8b9bad); overflow-wrap: anywhere; }
    .delete { min-height: 30px; padding: 0.2rem 0.5rem; color: var(--err, #f87171);
              border: 1px solid var(--panel-border, #223041); border-radius: 7px;
              background: var(--panel-2, #0b121b); font: inherit; cursor: pointer; }
    .delete:hover { border-color: var(--err, #f87171); }
    .delete:disabled { opacity: 0.55; cursor: progress; }
    .rule .confirm { grid-column: 1 / -1; }

    .confirm {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.4rem;
      padding: 0.4rem 0.5rem;
      border: 1px solid rgba(248, 113, 113, 0.4);
      border-radius: 7px;
      background: rgba(248, 113, 113, 0.06);
      font-size: 12px;
    }
    .confirm > span { margin-right: auto; }

    .muted { color: var(--muted, #8b9bad); }
    .empty,
    .error,
    .notice {
      margin: 0;
      padding: 0.75rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
    }
    .empty { color: var(--muted, #8b9bad); }
    .error { color: var(--err, #f87171); }
    .notice { color: var(--ok, #34d399); border-color: rgba(52, 211, 153, 0.35); }

    .raw {
      padding: 0.5rem 0.65rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel-2, #0b121b);
      font: 12px/1.4 var(--mono, monospace);
    }
    .raw summary { cursor: pointer; color: var(--muted, #8b9bad); }
    .raw pre { margin: 0.5rem 0 0; overflow-x: auto; white-space: pre-wrap; }

    @media (max-width: 44rem) {
      .toolbar button { min-height: 44px; }
      .toolbar .btn-label { display: inline; }
      .add { grid-template-columns: 1fr 1fr; }
      .add .grow { grid-column: 1 / -1; }
      .add input,
      .add select,
      .add button { min-height: 44px; }
      .add button { grid-column: 1 / -1; }
      .rule { grid-template-columns: auto auto 1fr auto; }
      .rule .comment { grid-column: 1 / -1; }
      .rule .delete { min-height: 44px; }
      .confirm button { min-height: 44px; }
    }
  `
}
