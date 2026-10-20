import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import type {
  FirewallAction,
  FirewallClient,
  FirewallProtocol,
  FirewallRule,
  FirewallSnapshot,
} from '../lib/firewall/firewall-client'
import {
  actionLabel,
  parsePort,
  ruleComment,
  ruleCountLabel,
  ruleTarget,
} from '../lib/firewall/display'

/**
 * Firewall manager backed by the daemon's managed `inet anthracite` table.
 *
 * It depends on the injected `FirewallClient`; the HTTP implementation lives in
 * `lib/firewall/firewall-client.ts`.
 */
@customElement('anth-firewall')
export class AnthFirewall extends LitElement {
  @property({ attribute: false })
  client?: FirewallClient

  @property({ type: Boolean, attribute: 'auto-load' })
  autoLoad = true

  @state()
  private snapshot?: FirewallSnapshot

  @state()
  private loading = false

  @state()
  private busy = false

  @state()
  private errorMessage = ''

  @state()
  private notice = ''

  @state()
  private action: FirewallAction = 'allow'

  @state()
  private port = ''

  @state()
  private protocol: FirewallProtocol = 'tcp'

  @state()
  private comment = ''

  protected override willUpdate(changed: PropertyValues<this>): void {
    if (
      this.client &&
      this.autoLoad &&
      (changed.has('client') || (changed.has('autoLoad') && this.autoLoad))
    ) {
      void this.refresh()
    }
  }

  /** Fetches the current managed table without changing it. */
  async refresh(): Promise<void> {
    // Do not race a mutation: the stale snapshot would make the table flicker.
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

  private onActionChange(event: Event): void {
    this.action = (event.target as HTMLSelectElement).value as FirewallAction
  }

  private onProtocolChange(event: Event): void {
    this.protocol = (event.target as HTMLSelectElement)
      .value as FirewallProtocol
  }

  private onPortInput(event: Event): void {
    this.port = (event.target as HTMLInputElement).value
  }

  private onCommentInput(event: Event): void {
    this.comment = (event.target as HTMLInputElement).value
  }

  private async addRule(event: Event): Promise<void> {
    event.preventDefault()
    if (!this.client || !this.canControl || this.busy) return

    const port = parsePort(this.port)
    if (port === null) {
      this.errorMessage = 'Port must be a number between 1 and 65535.'
      return
    }
    const comment = this.comment.trim()
    if (comment !== '' && !/^[A-Za-z0-9._-]{1,32}$/.test(comment)) {
      this.errorMessage =
        'Comment must be at most 32 characters and use only letters, digits, dot, underscore or dash.'
      return
    }

    this.busy = true
    this.errorMessage = ''
    this.notice = ''
    let changed = false
    try {
      this.notice = await this.client.addRule({
        action: this.action,
        port,
        protocol: this.protocol,
        comment: comment || undefined,
      })
      this.port = ''
      this.comment = ''
      changed = true
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.busy = false
    }

    if (changed) await this.refresh()
  }

  private async deleteRule(rule: FirewallRule): Promise<void> {
    if (!this.client || !this.canControl || this.busy) return
    if (
      !window.confirm(
        `Delete ${actionLabel(rule.action)} ${ruleTarget(rule)} (handle ${rule.handle})?`,
      )
    ) {
      return
    }

    this.busy = true
    this.errorMessage = ''
    this.notice = ''
    let changed = false
    try {
      this.notice = await this.client.deleteRule(rule.handle)
      changed = true
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.busy = false
    }

    if (changed) await this.refresh()
  }

  private async flush(): Promise<void> {
    if (!this.client || !this.canControl || this.busy) return
    if (!window.confirm('Remove every rule from the managed firewall chain?')) {
      return
    }

    this.busy = true
    this.errorMessage = ''
    this.notice = ''
    let changed = false
    try {
      this.notice = await this.client.flush()
      changed = true
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.busy = false
    }

    if (changed) await this.refresh()
  }

  private renderForm() {
    return html`
      <div class="form-wrap">
        <form class="add-rule" part="form" @submit=${this.addRule}>
          <select
            aria-label="Firewall action"
            .value=${this.action}
            @change=${this.onActionChange}
          >
            <option value="allow">Allow</option>
            <option value="deny">Deny</option>
          </select>
          <input
            type="number"
            min="1"
            max="65535"
            inputmode="numeric"
            placeholder="Port"
            aria-label="Port"
            .value=${this.port}
            @input=${this.onPortInput}
          />
          <select
            aria-label="Protocol"
            .value=${this.protocol}
            @change=${this.onProtocolChange}
          >
            <option value="tcp">tcp</option>
            <option value="udp">udp</option>
          </select>
          <input
            type="text"
            maxlength="32"
            placeholder="Comment (optional)"
            aria-label="Comment"
            .value=${this.comment}
            @input=${this.onCommentInput}
          />
          <button type="submit" ?disabled=${this.busy}>Add rule</button>
        </form>
        <p class="hint">
          Deny adds a drop rule. Allow removes a matching managed deny.
        </p>
      </div>
    `
  }

  private renderRules() {
    const snapshot = this.snapshot
    if (!snapshot) {
      return html`<p class="empty">
        ${this.loading ? 'Loading firewall…' : 'No firewall snapshot yet.'}
      </p>`
    }
    if (!snapshot.initialized) {
      return html`<p class="empty">
        The managed table does not exist. Adding a rule creates it.
      </p>`
    }
    if (snapshot.rules.length === 0) {
      return html`<p class="empty">No rules in the managed chain.</p>`
    }

    return html`
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Handle</th>
              <th>Action</th>
              <th>Target</th>
              <th>Comment</th>
              <th class="actions-head">Control</th>
            </tr>
          </thead>
          <tbody>
            ${snapshot.rules.map(
              (rule) => html`
                <tr>
                  <td class="mono">${rule.handle}</td>
                  <td>
                    <span class="action" data-action=${rule.action}>
                      ${actionLabel(rule.action)}
                    </span>
                  </td>
                  <td class="mono">${ruleTarget(rule)}</td>
                  <td>${ruleComment(rule)}</td>
                  <td class="actions">
                    ${this.canControl
                      ? html`
                          <button
                            type="button"
                            class="danger"
                            ?disabled=${this.busy}
                            @click=${() => this.deleteRule(rule)}
                          >
                            Delete
                          </button>
                        `
                      : nothing}
                  </td>
                </tr>
              `,
            )}
          </tbody>
        </table>
      </div>
    `
  }

  override render() {
    const snapshot = this.snapshot
    return html`
      <section class="firewall" part="firewall" aria-busy=${this.loading || this.busy}>
        <header class="toolbar" part="toolbar">
          <div class="status">
            <span class="dot" data-ready=${snapshot?.initialized ?? false}></span>
            <span>
              ${snapshot
                ? snapshot.initialized
                  ? 'managed table ready'
                  : 'managed table not initialized'
                : 'no snapshot'}
            </span>
            ${snapshot
              ? html`<span class="muted"
                  >${ruleCountLabel(snapshot)}</span
                >`
              : nothing}
          </div>
          <span class="spacer"></span>
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
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${this.flush}
                >
                  Flush rules
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
        ${snapshot?.warning
          ? html`<p class="warning" part="warning" role="status">
              ${snapshot.warning}
            </p>`
          : nothing}

        ${this.canControl
          ? this.renderForm()
          : html`<p class="control-off">
              Firewall control is disabled by the daemon.
            </p>`}

        ${this.renderRules()}

        ${snapshot?.ruleset
          ? html`
              <details class="ruleset">
                <summary>nft ruleset</summary>
                <pre>${snapshot.ruleset}</pre>
              </details>
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

    .firewall {
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
    .add-rule {
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
    select:focus-visible,
    input:focus-visible,
    summary:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .danger {
      color: #fca5a5;
    }

    .form-wrap {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    .hint {
      margin: 0;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .add-rule {
      padding: 9px 10px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: #0c131c;
    }

    .add-rule input[type='number'] {
      width: 7rem;
    }

    .add-rule input[type='text'] {
      flex: 1;
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

    th:nth-child(1),
    td:nth-child(1) {
      width: 6rem;
    }

    th:nth-child(2),
    td:nth-child(2) {
      width: 7rem;
    }

    th:nth-child(3),
    td:nth-child(3) {
      width: 8rem;
    }

    th:nth-child(4),
    td:nth-child(4) {
      width: auto;
    }

    th:nth-child(5),
    td:nth-child(5) {
      width: 6rem;
    }

    th {
      color: var(--muted, #8b9bad);
      font-size: 0.78em;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }

    tbody tr:last-child td {
      border-bottom: 0;
    }

    .mono {
      font-variant-numeric: tabular-nums;
    }

    .action {
      display: inline-flex;
      padding: 1px 6px;
      border-radius: 999px;
      font-size: 0.82em;
      letter-spacing: 0.04em;
    }

    .action[data-action='allow'] {
      background: rgba(52, 211, 153, 0.14);
      color: #6ee7b7;
    }

    .action[data-action='deny'] {
      background: rgba(248, 113, 113, 0.14);
      color: #fca5a5;
    }

    .action[data-action='custom'] {
      background: rgba(251, 191, 36, 0.14);
      color: #fcd34d;
    }

    .actions,
    .actions-head {
      text-align: right;
    }

    .actions button {
      padding: 2px 7px;
      font-size: 0.85em;
    }

    .empty {
      margin: 0;
      padding: 18px 10px;
      color: var(--muted, #8b9bad);
      text-align: center;
      border: 1px dashed var(--panel-border, #223041);
      border-radius: 8px;
    }

    .ruleset {
      border-top: 1px solid var(--panel-border, #223041);
      padding-top: 8px;
      color: var(--muted, #8b9bad);
      font: 12px/1.45 var(--mono, monospace);
    }

    .ruleset summary {
      cursor: pointer;
    }

    .ruleset pre {
      margin: 8px 0 0;
      overflow: auto;
      white-space: pre-wrap;
      color: var(--text, #dbe6f2);
    }
  `
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-firewall': AnthFirewall
  }
}
