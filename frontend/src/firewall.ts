/**
 * `anth-firewall`: the managed nftables table.
 *
 * Unlike the network inventory this panel does not stream: it reads on connect
 * and after every mutation. The daemon owns exactly `inet anthracite` with its
 * `input` and `output` base chains; the panel never offers another table.
 *
 * Both chains keep accept and drop rules and have their own default policy.
 * One managed rule exists per protocol/port in a chain: adding a rule for a
 * target that is already managed replaces it.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import { describeError } from './errors'
import type {
  FirewallAction,
  FirewallChain,
  FirewallChainState,
  FirewallClient,
  FirewallProtocol,
  FirewallRule,
  FirewallRuleAction,
  FirewallSnapshot,
} from './firewall-client'

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error'

const CHAINS: FirewallChain[] = ['input', 'output']

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
  private chain: FirewallChain = 'input'

  @state()
  private protocol: FirewallProtocol = 'tcp'

  @state()
  private port = ''

  @state()
  private comment = ''

  @state()
  private confirmingFlush?: FirewallChain

  @state()
  private confirmingDelete?: number

  @state()
  private editing?: number

  @state()
  private editAction: FirewallAction = 'deny'

  @state()
  private editChain: FirewallChain = 'input'

  @state()
  private editProtocol: FirewallProtocol = 'tcp'

  @state()
  private editPort = ''

  @state()
  private editComment = ''

  @state()
  private policyDrafts: Partial<Record<FirewallChain, FirewallAction>> = {}

  @state()
  private pendingPolicy?: { chain: FirewallChain; policy: FirewallAction }

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
      this.error = this.failureMessage(failure)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private failureMessage(failure: unknown): string {
    if ((failure as { status?: number }).status === 405) {
      return msg(
        'The daemon does not support this firewall operation. Rebuild and restart it, then reload the page.',
      )
    }
    return describeError(failure)
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
      this.error = this.failureMessage(failure)
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
      (current) =>
        current.add(
          this.chain,
          this.action,
          this.protocol,
          port,
          comment || undefined,
        ),
      () => {
        this.port = ''
        this.comment = ''
      },
    )
  }

  private beginEdit(rule: FirewallRule): void {
    if (rule.action === 'custom') return
    this.editing = rule.handle
    this.editAction = rule.action
    this.editChain = rule.chain
    this.editProtocol = rule.protocol ?? 'tcp'
    this.editPort = rule.port ? String(rule.port) : ''
    this.editComment = rule.comment ?? ''
    this.confirmingDelete = undefined
  }

  private submitEdit(event: Event, rule: FirewallRule): void {
    event.preventDefault()
    const client = this.client
    if (!client || this.busy) return
    const port = Number(this.editPort)
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      this.error = msg('The port must be between 1 and 65535.')
      return
    }
    const comment = this.editComment.trim()
    void this.run(
      (current) =>
        current.update(
          rule.handle,
          this.editChain,
          this.editAction,
          this.editProtocol,
          port,
          comment || undefined,
        ),
      () => {
        this.chain = this.editChain
        this.editing = undefined
      },
    )
  }

  private deleteRule(rule: FirewallRule): void {
    void this.run(
      (client) => client.remove(rule.handle),
      () => {
        this.confirmingDelete = undefined
        if (this.editing === rule.handle) this.editing = undefined
      },
    )
  }

  private flush(chain: FirewallChain): void {
    void this.run(
      (client) => client.flush(chain),
      () => {
        this.confirmingFlush = undefined
      },
    )
  }

  private policyDraft(state: FirewallChainState): FirewallAction {
    return this.policyDrafts[state.chain] ?? state.policy
  }

  private setPolicyDraft(chain: FirewallChain, policy: FirewallAction): void {
    this.policyDrafts = { ...this.policyDrafts, [chain]: policy }
    this.pendingPolicy = undefined
  }

  private requestPolicy(state: FirewallChainState): void {
    const policy = this.policyDraft(state)
    if (policy === state.policy) return
    this.pendingPolicy = { chain: state.chain, policy }
    this.confirmingFlush = undefined
  }

  private confirmPolicy(): void {
    const pending = this.pendingPolicy
    if (!pending) return
    void this.run(
      (client) => client.setPolicy(pending.chain, pending.policy),
      () => {
        this.pendingPolicy = undefined
        this.policyDrafts = {}
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
    const count = this.chainRules().length
    if (count === 0) return msg('No rules')
    if (count === 1) return msg('1 rule')
    return msg(str`${count} rules`)
  }

  private chainRules(): FirewallRule[] {
    return (this.snapshot?.rules ?? []).filter((rule) => rule.chain === this.chain)
  }

  private chainCount(chain: FirewallChain): number {
    return (this.snapshot?.rules ?? []).filter((rule) => rule.chain === chain).length
  }

  private selectChain(chain: FirewallChain): void {
    if (this.chain === chain) return
    this.chain = chain
    this.editing = undefined
    this.confirmingDelete = undefined
    this.confirmingFlush = undefined
    this.pendingPolicy = undefined
  }

  private renderTabs() {
    const chains = this.snapshot?.chains ?? []
    if (chains.length === 0) return nothing
    return html`
      <div class="subtabs" role="tablist" part="subtabs">
        ${chains.map((state) => {
          const active = this.chain === state.chain
          return html`
            <button
              type="button"
              class="subtab"
              role="tab"
              data-chain=${state.chain}
              data-active=${active ? '' : nothing}
              aria-selected=${active ? 'true' : 'false'}
              @click=${() => this.selectChain(state.chain)}
            >
              <span>${state.chain === 'input' ? msg('Input') : msg('Output')}</span>
              <span class="tab-count">${this.chainCount(state.chain)}</span>
            </button>
          `
        })}
      </div>
    `
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
      </div>
    `
  }

  private renderContext() {
    return html`
      <div class="context" part="context">
        <span class="context-label">${msg('Managed table')}</span>
        <span class="table-name">inet anthracite</span>
        <span class="muted">input + output</span>
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
        ${msg('Adding a rule for an existing protocol and port in this direction replaces it.')}
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

  private renderPolicyConfirm() {
    const pending = this.pendingPolicy
    if (!pending) return nothing
    return html`
      <div class="confirm policy-confirm" role="alert">
        <span>
          ${pending.policy === 'deny'
            ? msg(
                str`Set the ${pending.chain} policy to deny? Unmatched traffic will be dropped, which can lock you out.`,
              )
            : msg(str`Set the ${pending.chain} policy to allow?`)}
        </span>
        <button
          type="button"
          ?disabled=${this.busy}
          @click=${() => this.confirmPolicy()}
        >
          ${msg('Apply')}
        </button>
        <button
          type="button"
          ?disabled=${this.busy}
          @click=${() => {
            this.pendingPolicy = undefined
          }}
        >
          ${msg('Cancel')}
        </button>
      </div>
    `
  }

  private renderFlushConfirm(chain: FirewallChain) {
    return html`
      <div class="confirm" role="alert">
        <span>${msg(str`Flush every rule from the ${chain} chain?`)}</span>
        <button
          type="button"
          class="danger"
          ?disabled=${this.busy}
          @click=${() => this.flush(chain)}
        >
          ${msg('Flush')}
        </button>
        <button
          type="button"
          ?disabled=${this.busy}
          @click=${() => {
            this.confirmingFlush = undefined
          }}
        >
          ${msg('Cancel')}
        </button>
      </div>
    `
  }

  private renderChain(state: FirewallChainState) {
    const draft = this.policyDraft(state)
    const pending = this.pendingPolicy?.chain === state.chain
    return html`
      <div class="chain-row" part="chain" data-chain=${state.chain}>
        <span class="chain-name">${state.chain}</span>
        <span class="chain-state" data-on=${state.initialized ? 'yes' : 'no'}>
          ${state.initialized ? msg('managed') : msg('not created')}
        </span>
        <span class="policy-label">${msg('Policy')}</span>
        ${this.controlEnabled()
          ? html`
              <select
                .value=${draft}
                ?disabled=${this.busy}
                @change=${(event: Event) =>
                  this.setPolicyDraft(
                    state.chain,
                    (event.currentTarget as HTMLSelectElement)
                      .value as FirewallAction,
                  )}
              >
                <option value="allow">${msg('Allow')}</option>
                <option value="deny">${msg('Deny')}</option>
              </select>
              <button
                type="button"
                ?disabled=${this.busy || draft === state.policy}
                @click=${() => this.requestPolicy(state)}
              >
                ${msg('Apply')}
              </button>
              <button
                type="button"
                class="danger"
                ?disabled=${this.busy || !state.initialized}
                @click=${() => {
                  this.confirmingFlush =
                    this.confirmingFlush === state.chain
                      ? undefined
                      : state.chain
                  this.pendingPolicy = undefined
                }}
              >
                ${msg('Flush')}
              </button>
            `
          : html`<span class="policy-value">${this.actionLabel(state.policy)}</span>`}
      </div>
      ${this.confirmingFlush === state.chain
        ? this.renderFlushConfirm(state.chain)
        : nothing}
      ${pending ? this.renderPolicyConfirm() : nothing}
    `
  }

  private renderActiveChain() {
    const chains = this.snapshot?.chains ?? []
    if (chains.length === 0) {
      return html`<p class="error" role="alert">
        ${msg('This daemon does not report the input and output chains. Rebuild and restart it, then reload the page.')}
      </p>`
    }
    const state =
      chains.find((candidate) => candidate.chain === this.chain) ?? chains[0]
    const rules = this.chainRules()
    return html`
      ${this.renderChain(state)}
      ${this.controlEnabled() ? this.renderAddForm() : nothing}
      ${!this.snapshot?.initialized
        ? html`
            <p class="empty">
              ${msg('The managed table does not exist yet. Adding a rule or changing a policy creates it.')}
            </p>
          `
        : rules.length === 0
          ? html`<p class="empty">${msg('No managed rules in this direction. Adding a rule creates one.')}</p>`
          : html`<div class="list" part="list">${rules.map((rule) => this.renderRule(rule))}</div>`}
    `
  }

  private renderEditForm(rule: FirewallRule) {
    return html`
      <form
        class="edit"
        part="edit"
        @submit=${(event: Event) => this.submitEdit(event, rule)}
      >
        <label>
          <span>${msg('Direction')}</span>
          <select
            .value=${this.editChain}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.editChain = (event.currentTarget as HTMLSelectElement)
                .value as FirewallChain
            }}
          >
            ${CHAINS.map(
              (chain) => html`<option value=${chain}>${chain}</option>`,
            )}
          </select>
        </label>
        <label>
          <span>${msg('Action')}</span>
          <select
            .value=${this.editAction}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.editAction = (event.currentTarget as HTMLSelectElement)
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
            .value=${this.editPort}
            ?disabled=${this.busy}
            @input=${(event: Event) => {
              this.editPort = (event.currentTarget as HTMLInputElement).value
            }}
          />
        </label>
        <label>
          <span>${msg('Protocol')}</span>
          <select
            .value=${this.editProtocol}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.editProtocol = (event.currentTarget as HTMLSelectElement)
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
            .value=${this.editComment}
            ?disabled=${this.busy}
            @input=${(event: Event) => {
              this.editComment = (event.currentTarget as HTMLInputElement).value
            }}
          />
        </label>
        <button type="submit" ?disabled=${this.busy}>${msg('Save')}</button>
        <button
          type="button"
          ?disabled=${this.busy}
          @click=${() => {
            this.editing = undefined
          }}
        >
          ${msg('Cancel')}
        </button>
      </form>
    `
  }

  private renderRule(rule: FirewallRule) {
    return html`
      <article class="rule" part="rule" data-action=${rule.action}>
        <span class="handle">#${rule.handle}</span>
        <span class="badge">${this.actionLabel(rule.action)}</span>
        <span class="target">${this.ruleTarget(rule)}</span>
        <span class="comment">${rule.comment || '—'}</span>
        ${this.controlEnabled() && rule.action !== 'custom'
          ? html`
              <button
                type="button"
                class="edit-rule"
                ?disabled=${this.busy}
                title=${msg(str`Edit rule #${rule.handle}`)}
                @click=${() => this.beginEdit(rule)}
              >
                ${msg('Edit')}
              </button>
            `
          : nothing}
        ${this.controlEnabled()
          ? html`
              <button
                type="button"
                class="danger delete"
                ?disabled=${this.busy}
                title=${msg(str`Delete rule #${rule.handle}`)}
                @click=${() => {
                  this.confirmingDelete =
                    this.confirmingDelete === rule.handle
                      ? undefined
                      : rule.handle
                  this.editing = undefined
                }}
              >
                ${msg('Delete')}
              </button>
            `
          : nothing}
        ${this.editing === rule.handle ? this.renderEditForm(rule) : nothing}
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

    return html`
      ${this.error ? html`<p class="error" role="alert">${this.error}</p>` : nothing}
      ${this.notice ? html`<p class="notice" role="status">${this.notice}</p>` : nothing}
      ${this.renderContext()} ${this.renderTabs()} ${this.renderActiveChain()}
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
    .confirm button,
    .chain-row button {
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
    .confirm button:hover,
    .chain-row button:hover { border-color: var(--accent, #7dd3fc); }
    .toolbar button:disabled,
    .confirm button:disabled,
    .chain-row button:disabled { opacity: 0.55; cursor: progress; }
    .toolbar button.danger,
    .confirm button.danger,
    .chain-row button.danger { color: var(--err, #f87171); }
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

    .subtabs {
      display: flex;
      flex-wrap: nowrap;
      gap: 0.35rem;
      margin-bottom: 0.25rem;
      overflow-x: auto;
      overscroll-behavior-x: contain;
      -webkit-overflow-scrolling: touch;
      scrollbar-width: thin;
      border-bottom: 1px solid var(--panel-border, #223041);
    }
    .subtab {
      display: inline-flex;
      flex: 0 0 auto;
      align-items: center;
      gap: 0.4rem;
      min-height: 40px;
      padding: 0.4rem 0.8rem;
      color: var(--muted, #8b9bad);
      border: 0;
      border-bottom: 2px solid transparent;
      background: transparent;
      font: 600 12px/1.3 var(--mono, monospace);
      letter-spacing: 0.02em;
      white-space: nowrap;
      cursor: pointer;
    }
    .subtab:hover,
    .subtab:focus-visible { color: var(--text, #dbe6f2); outline: none; }
    .subtab:focus-visible {
      box-shadow: inset 0 0 0 1px var(--accent, #7dd3fc);
      border-radius: 4px;
    }
    .subtab[data-active] {
      color: var(--accent, #7dd3fc);
      border-bottom-color: var(--accent, #7dd3fc);
    }
    .tab-count {
      min-width: 1.25rem;
      padding: 0 0.3rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font-size: 10px;
      text-align: center;
    }
    .subtab[data-active] .tab-count {
      color: var(--accent, #7dd3fc);
      border-color: var(--accent, #7dd3fc);
    }

    .chain-row {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.5rem 0.75rem;
      padding: 0.5rem 0.65rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
      font: 12px/1.4 var(--mono, monospace);
    }
    .chain-name {
      min-width: 3.5rem;
      font-weight: 700;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
    .chain-state {
      padding: 0.1rem 0.4rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font-size: 10px;
      text-transform: uppercase;
    }
    .chain-state[data-on='yes'] { color: var(--ok, #34d399); }
    .policy-label { color: var(--muted, #8b9bad); }
    .policy-value { color: var(--text, #dbe6f2); font-weight: 600; }
    .chain-row select {
      min-height: 30px;
      padding: 0.2rem 0.45rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 12px/1.4 var(--mono, monospace);
    }
    .chain-row .confirm { flex-basis: 100%; }

    .add,
    .edit {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr));
      align-items: end;
      gap: 0.5rem 0.75rem;
      padding: 0.65rem 0.75rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .edit { grid-column: 1 / -1; background: var(--panel-2, #0b121b); }
    .add label,
    .edit label {
      display: grid;
      gap: 0.2rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }
    .add .grow,
    .edit .grow { grid-column: span 2; }
    .add input,
    .add select,
    .edit input,
    .edit select {
      min-height: 32px;
      padding: 0.3rem 0.5rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
    }
    .add button,
    .edit button {
      min-height: 32px;
      padding: 0.3rem 0.65rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .add button:hover,
    .edit button:hover { border-color: var(--accent, #7dd3fc); }
    .add button:disabled,
    .edit button:disabled { opacity: 0.55; cursor: progress; }
    .add input:focus-visible,
    .add select:focus-visible,
    .add button:focus-visible,
    .edit input:focus-visible,
    .edit select:focus-visible,
    .edit button:focus-visible,
    .chain-row select:focus-visible,
    .chain-row button:focus-visible,
    .toolbar button:focus-visible,
    .confirm button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    .hint { margin: -0.25rem 0 0; color: var(--muted, #8b9bad); font-size: 11px; }

    .list { display: flex; flex-direction: column; gap: 0.5rem; }
    .rule {
      display: grid;
      grid-template-columns: auto auto minmax(5rem, auto) 1fr auto auto;
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
    .edit-rule,
    .delete {
      min-height: 30px;
      padding: 0.2rem 0.5rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .delete { color: var(--err, #f87171); }
    .edit-rule:hover,
    .delete:hover { border-color: var(--accent, #7dd3fc); }
    .delete:hover { border-color: var(--err, #f87171); }
    .edit-rule:disabled,
    .delete:disabled { opacity: 0.55; cursor: progress; }
    .rule .confirm,
    .rule form.edit { grid-column: 1 / -1; }

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
      .add,
      .edit { grid-template-columns: 1fr 1fr; }
      .add .grow,
      .edit .grow { grid-column: 1 / -1; }
      .add input,
      .add select,
      .add button,
      .edit input,
      .edit select,
      .edit button { min-height: 44px; }
      .add button,
      .edit button { grid-column: 1 / -1; }
      .rule { grid-template-columns: auto auto 1fr auto auto; }
      .rule .handle { grid-area: 1 / 1; }
      .rule .badge { grid-area: 1 / 2; }
      .rule .target { grid-area: 1 / 3; }
      .rule .comment { grid-area: 2 / 1 / 3 / -1; }
      .rule .edit-rule { grid-area: 1 / 4; }
      .rule .delete { grid-area: 1 / 5; }
      .rule .edit-rule,
      .rule .delete { min-height: 44px; }
      .subtab { min-height: 44px; }
      .chain-row button,
      .chain-row select { min-height: 44px; }
      .confirm button { min-height: 44px; }
    }
  `
}
