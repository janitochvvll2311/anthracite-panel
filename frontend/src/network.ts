/**
 * `anth-network`: the host network inventory, live.
 *
 * The component owns its feed: it subscribes to `/api/network/watch` and
 * repaints on every snapshot. Rates are derived here from two consecutive
 * snapshots, because the daemon reports cumulative counters and never guesses
 * an interval.
 *
 * This slice is read-only. Up/down and the create/edit/rename/delete forms
 * arrive with `[network] control` and `[network] manage`; until then the
 * component only reports the policy flags it receives.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import { describeError } from './errors'
import { formatBytes } from './format'
import {
  computeRows,
  countUp,
  filterRows,
  formatAddress,
  formatRate,
  formatSpeed,
  hasErrors,
} from './lib/network/display'
import type { NetworkRow } from './lib/network/display'
import type {
  NetworkClient,
  NetworkCreateKind,
  NetworkCreateRequest,
  NetworkSnapshot,
} from './network-client'

/** The intervals the selector offers, in milliseconds. */
const INTERVALS = [1000, 2000, 5000, 10000, 30000] as const

/** The virtual kinds the create form offers, in display order. */
const CREATE_KINDS: NetworkCreateKind[] = [
  'bridge',
  'bond',
  'dummy',
  'veth',
  'vlan',
  'tun',
]

function kindLabel(kind: NetworkCreateKind): string {
  return kind === 'tun' ? 'tun/tap' : kind
}

type FeedStatus = 'idle' | 'connecting' | 'live' | 'paused' | 'stopped' | 'error'

@customElement('anth-network')
@localized()
export class AnthNetwork extends LitElement {
  @property({ attribute: false })
  client?: NetworkClient

  @state()
  private snapshot?: NetworkSnapshot

  @state()
  private rows: NetworkRow[] = []

  @state()
  private status: FeedStatus = 'idle'

  @state()
  private error = ''

  @state()
  private interval: number = INTERVALS[1]

  @state()
  private paused = false

  @state()
  private query = ''

  @state()
  private busy = false

  @state()
  private actionError = ''

  @state()
  private notice = ''

  @state()
  private createOpen = false

  @state()
  private createKind: NetworkCreateKind = 'bridge'

  @state()
  private editing?: string

  @state()
  private renaming?: string

  @state()
  private confirmingDelete?: string

  private previous?: NetworkSnapshot

  private unsubscribe?: () => void

  override connectedCallback(): void {
    super.connectedCallback()
    this.start()
  }

  /** A parked tab closes its stream; returning starts it again. */
  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.stop()
  }

  private start(): void {
    this.stop()
    if (!this.client) {
      this.status = 'idle'
      return
    }
    this.paused = false
    this.status = 'connecting'
    this.error = ''
    this.notice = ''
    this.actionError = ''
    this.previous = undefined
    this.unsubscribe = this.client.watch(this.interval, {
      onSnapshot: (snapshot) => {
        this.rows = computeRows(snapshot, this.previous)
        this.previous = snapshot
        this.snapshot = snapshot
        this.error = ''
        this.status = 'live'
      },
      onError: (error) => {
        this.error = describeError(error)
        this.status = 'error'
      },
      onStatus: (status) => {
        if (status === 'connecting') {
          this.status = 'connecting'
        } else if (status === 'live') {
          this.status = 'live'
        } else if (this.status !== 'error') {
          this.status = 'stopped'
        }
      },
    })
  }

  private stop(): void {
    this.unsubscribe?.()
    this.unsubscribe = undefined
  }

  private pause(): void {
    this.stop()
    this.paused = true
    this.status = 'paused'
  }

  private resume(): void {
    this.start()
  }

  private toggleFeed(): void {
    if (this.paused || this.status === 'error' || this.status === 'stopped') {
      this.resume()
      return
    }
    this.pause()
  }

  private changeInterval(event: Event): void {
    this.interval = Number((event.currentTarget as HTMLSelectElement).value)
    if (!this.paused) {
      // SSE carries the schedule in the query, so a new period is a new URL.
      this.start()
    }
  }

  /** Runs one mutation, refreshes the rows and reports one generic success. */
  private async run(
    task: (client: NetworkClient) => Promise<unknown>,
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
      const snapshot = await client.get()
      this.rows = computeRows(snapshot, this.previous)
      this.previous = snapshot
      this.snapshot = snapshot
      this.error = ''
      this.notice = msg('Change applied.')
    } catch (error) {
      this.actionError = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private canManage(): boolean {
    return this.snapshot?.manage_enabled === true
  }

  private canControl(): boolean {
    return this.snapshot?.control_enabled === true
  }

  private stateBlockReason(iface: NetworkRow): string {
    if (!this.canControl()) return msg('Network control is disabled.')
    if (iface.loopback && iface.up) {
      return msg('Loopback interfaces cannot be brought down.')
    }
    return ''
  }

  private renameBlockReason(iface: NetworkRow): string {
    if (!this.canManage()) return msg('Network management is disabled.')
    if (iface.up) return msg('Bring the interface down first.')
    return ''
  }

  private deleteBlockReason(iface: NetworkRow): string {
    if (!this.canManage()) return msg('Network management is disabled.')
    if (iface.loopback) return msg('Loopback interfaces are protected.')
    const carriers = new Set(
      (this.snapshot?.routes ?? []).map((route) => route.interface),
    )
    if (carriers.has(iface.name) && carriers.size <= 1) {
      return msg('This is the last interface carrying the default route.')
    }
    return ''
  }

  private toggleState(iface: NetworkRow): void {
    if (this.stateBlockReason(iface) !== '') return
    void this.run((client) => client.setState(iface.name, !iface.up))
  }

  private submitMtu(event: Event, iface: NetworkRow): void {
    event.preventDefault()
    const form = event.currentTarget as HTMLFormElement
    const raw = String(new FormData(form).get('mtu') ?? '').trim()
    if (raw === '') {
      this.actionError = msg('The MTU is required.')
      return
    }
    void this.run(
      (client) => client.edit(iface.name, { mtu: Number(raw) }),
      () => {
        this.editing = undefined
      },
    )
  }

  private submitRename(event: Event, iface: NetworkRow): void {
    event.preventDefault()
    const form = event.currentTarget as HTMLFormElement
    const newName = String(new FormData(form).get('new_name') ?? '').trim()
    if (newName === '') {
      this.actionError = msg('The new name is required.')
      return
    }
    void this.run(
      (client) => client.rename(iface.name, newName),
      () => {
        this.renaming = undefined
      },
    )
  }

  private submitDelete(iface: NetworkRow): void {
    if (this.deleteBlockReason(iface) !== '') return
    void this.run(
      (client) => client.remove(iface.name),
      () => {
        this.confirmingDelete = undefined
      },
    )
  }

  private submitCreate(event: Event): void {
    event.preventDefault()
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const name = String(data.get('name') ?? '').trim()
    const kind = String(data.get('kind') ?? 'bridge') as NetworkCreateKind
    if (name === '') {
      this.actionError = msg('The interface name is required.')
      return
    }

    const request: NetworkCreateRequest = { name, kind, up: data.get('up') === 'on' }
    const mtu = String(data.get('mtu') ?? '').trim()
    if (mtu !== '') request.mtu = Number(mtu)
    if (kind === 'veth') {
      const peer = String(data.get('peer') ?? '').trim()
      if (peer === '') {
        this.actionError = msg('A veth needs a peer interface name.')
        return
      }
      request.peer = peer
    }
    if (kind === 'vlan') {
      const link = String(data.get('link') ?? '').trim()
      const vlanId = String(data.get('vlan_id') ?? '').trim()
      if (link === '' || vlanId === '') {
        this.actionError = msg('A vlan needs a parent interface and an identifier.')
        return
      }
      request.link = link
      request.vlan_id = Number(vlanId)
    }

    void this.run(
      (client) => client.create(request),
      () => {
        this.createOpen = false
      },
    )
  }

  private statusText(): string {
    switch (this.status) {
      case 'live':
        return msg('live')
      case 'connecting':
        return msg('connecting')
      case 'error':
        return msg('error')
      case 'paused':
        return msg('paused')
      case 'stopped':
        return msg('stopped')
      case 'idle':
        return msg('idle')
    }
  }

  private buttonLabel(): string {
    if (this.paused) return msg('Resume')
    if (this.status === 'error' || this.status === 'stopped') return msg('Retry')
    return msg('Pause')
  }

  private buttonIcon(): string {
    if (this.paused) return 'play'
    if (this.status === 'error' || this.status === 'stopped') return 'refresh'
    return 'pause'
  }

  private visibleRows(): NetworkRow[] {
    return filterRows(this.rows, this.query)
  }

  private renderToolbar() {
    return html`
      <div class="toolbar" part="toolbar">
        <span class="status" data-state=${this.status} role="status">${this.statusText()}</span>
        <span class="count">
          ${msg(
            str`${this.rows.length} interfaces · ${countUp(this.rows)} up`,
          )}
        </span>
        <label class="field">
          <span class="field-label">${msg('Refresh')}</span>
          <select
            aria-label=${msg('Refresh interval')}
            .value=${String(this.interval)}
            @change=${(event: Event) => this.changeInterval(event)}
          >
            ${INTERVALS.map(
              (ms) =>
                html`<option value=${String(ms)} ?selected=${ms === this.interval}>
                  ${msg('every')} ${intervalLabel(ms)}
                </option>`,
            )}
          </select>
        </label>
        ${this.canManage()
          ? html`
              <button
                type="button"
                aria-expanded=${this.createOpen ? 'true' : 'false'}
                title=${msg('Create a virtual interface')}
                @click=${() => {
                  this.createOpen = !this.createOpen
                }}
              >
                <anth-icon name="plus"></anth-icon>
                <span class="btn-label">${msg('Create')}</span>
              </button>
            `
          : nothing}
        <button
          type="button"
          aria-label=${this.buttonLabel()}
          title=${this.buttonLabel()}
          aria-pressed=${this.paused ? 'true' : 'false'}
          @click=${() => this.toggleFeed()}
        >
          <anth-icon name=${this.buttonIcon()}></anth-icon>
          <span class="btn-label">${this.buttonLabel()}</span>
        </button>
      </div>
    `
  }

  private renderRoutes() {
    const routes = this.snapshot?.routes ?? []
    return html`
      <div class="routes" part="routes">
        <span class="routes-label">${msg('Default route')}</span>
        ${routes.length === 0
          ? html`<span class="muted">${msg('none')}</span>`
          : routes.map(
              (route) => html`
                <span class="route" data-family=${route.family}>
                  <span class="family">${route.family}</span>
                  <span class="gateway">${route.gateway}</span>
                  <span class="muted">dev ${route.interface}</span>
                </span>
              `,
            )}
      </div>
    `
  }

  private renderCreateForm() {
    return html`
      <form class="create" part="create" @submit=${(event: Event) => this.submitCreate(event)}>
        <div class="create-grid">
          <label>
            <span>${msg('Name')}</span>
            <input name="name" required maxlength="15" placeholder="br0" ?disabled=${this.busy} />
          </label>
          <label>
            <span>${msg('Kind')}</span>
            <select
              name="kind"
              .value=${this.createKind}
              ?disabled=${this.busy}
              @change=${(event: Event) => {
                this.createKind = (event.currentTarget as HTMLSelectElement)
                  .value as NetworkCreateKind
              }}
            >
              ${CREATE_KINDS.map(
                (kind) => html`<option value=${kind}>${kindLabel(kind)}</option>`,
              )}
            </select>
          </label>
          ${this.createKind === 'veth'
            ? html`
                <label>
                  <span>${msg('Peer')}</span>
                  <input name="peer" maxlength="15" placeholder="veth1" ?disabled=${this.busy} />
                </label>
              `
            : nothing}
          ${this.createKind === 'vlan'
            ? html`
                <label>
                  <span>${msg('Parent interface')}</span>
                  <input name="link" maxlength="15" placeholder="eth0" ?disabled=${this.busy} />
                </label>
                <label>
                  <span>${msg('VLAN identifier')}</span>
                  <input name="vlan_id" type="number" min="1" max="4094" placeholder="20" ?disabled=${this.busy} />
                </label>
              `
            : nothing}
          <label>
            <span>${msg('MTU')}</span>
            <input name="mtu" type="number" min="68" max="65535" placeholder="1500" ?disabled=${this.busy} />
          </label>
          <label class="check">
            <input type="checkbox" name="up" ?disabled=${this.busy} />
            <span>${msg('Bring up')}</span>
          </label>
        </div>
        <div class="create-actions">
          <button type="submit" ?disabled=${this.busy}>${msg('Create')}</button>
          <button type="button" ?disabled=${this.busy} @click=${() => { this.createOpen = false }}>
            ${msg('Cancel')}
          </button>
        </div>
        <p class="hint">${msg('Only virtual interfaces can be created. Addresses are not assigned here.')}</p>
      </form>
    `
  }

  private editMtu(iface: NetworkRow) {
    return html`
      <form class="inline" @submit=${(event: Event) => this.submitMtu(event, iface)}>
        <label>
          <span>${msg('MTU')}</span>
          <input
            name="mtu"
            type="number"
            min="68"
            max="65535"
            .value=${String(iface.mtu)}
            ?disabled=${this.busy}
            required
          />
        </label>
        <button type="submit" ?disabled=${this.busy}>${msg('Apply')}</button>
        <button type="button" ?disabled=${this.busy} @click=${() => { this.editing = undefined }}>
          ${msg('Cancel')}
        </button>
      </form>
    `
  }

  private renameForm(iface: NetworkRow) {
    return html`
      <form class="inline" @submit=${(event: Event) => this.submitRename(event, iface)}>
        <label>
          <span>${msg('New name')}</span>
          <input
            name="new_name"
            maxlength="15"
            .value=${iface.name}
            ?disabled=${this.busy}
            required
          />
        </label>
        <button type="submit" ?disabled=${this.busy}>${msg('Rename')}</button>
        <button type="button" ?disabled=${this.busy} @click=${() => { this.renaming = undefined }}>
          ${msg('Cancel')}
        </button>
      </form>
    `
  }

  private renderActions(iface: NetworkRow) {
    if (!this.canControl() && !this.canManage()) {
      return nothing
    }
    const stateReason = this.stateBlockReason(iface)
    const renameReason = this.renameBlockReason(iface)
    const deleteReason = this.deleteBlockReason(iface)
    return html`
      <div class="manage">
        <div class="actions">
          ${this.canControl()
            ? html`
                <button
                  type="button"
                  class=${iface.up ? 'danger' : ''}
                  title=${stateReason}
                  ?disabled=${this.busy || stateReason !== ''}
                  @click=${() => this.toggleState(iface)}
                >
                  ${iface.up ? msg('Down') : msg('Up')}
                </button>
              `
            : nothing}
          ${this.canManage()
            ? html`
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.editing = this.editing === iface.name ? undefined : iface.name
                    this.renaming = undefined
                    this.confirmingDelete = undefined
                  }}
                >
                  ${msg('Edit MTU')}
                </button>
                <button
                  type="button"
                  title=${renameReason}
                  ?disabled=${this.busy || renameReason !== ''}
                  @click=${() => {
                    this.renaming = this.renaming === iface.name ? undefined : iface.name
                    this.editing = undefined
                    this.confirmingDelete = undefined
                  }}
                >
                  ${msg('Rename')}
                </button>
                <button
                  type="button"
                  class="danger"
                  title=${deleteReason}
                  ?disabled=${this.busy || deleteReason !== ''}
                  @click=${() => {
                    this.confirmingDelete =
                      this.confirmingDelete === iface.name ? undefined : iface.name
                    this.editing = undefined
                    this.renaming = undefined
                  }}
                >
                  ${msg('Delete')}
                </button>
              `
            : nothing}
        </div>
        ${this.editing === iface.name ? this.editMtu(iface) : nothing}
        ${this.renaming === iface.name ? this.renameForm(iface) : nothing}
        ${this.confirmingDelete === iface.name
          ? html`
              <div class="confirm" role="alert">
                <span>
                  ${msg(str`Delete interface ${iface.name}? This cannot be undone.`)}
                </span>
                ${iface.physical
                  ? html`<span class="warning">
                      ${msg('This is a physical device; the host will lose this link.')}
                    </span>`
                  : nothing}
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => this.submitDelete(iface)}
                >
                  ${msg('Delete')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => { this.confirmingDelete = undefined }}
                >
                  ${msg('Cancel')}
                </button>
              </div>
            `
          : nothing}
      </div>
    `
  }

  private renderAddresses(iface: NetworkRow) {
    if (iface.addresses.length === 0) {
      return html`<p class="no-address">${msg('No addresses')}</p>`
    }
    return html`
      <ul class="addresses">
        ${iface.addresses.map(
          (address) => html`
            <li data-family=${address.family}>
              <span class="family">${address.family}</span>
              <span class="address">${formatAddress(address)}</span>
              <span class="scope">${scopeLabel(address.scope)}</span>
            </li>
          `,
        )}
      </ul>
    `
  }

  private renderCard(iface: NetworkRow) {
    return html`
      <article class="iface" part="iface" data-up=${iface.up ? 'yes' : 'no'}>
        <div class="identity">
          <div class="title">
            <span class="dot" aria-hidden="true"></span>
            <h3>${iface.name}</h3>
            <span class="kind">${iface.kind}</span>
            <span class="state">${iface.state}</span>
          </div>
          <p class="meta">
            <span>${msg('MTU')} ${iface.mtu}</span>
            ${iface.mac ? html`<span class="mac">${iface.mac}</span>` : nothing}
            ${iface.speed_mbps
              ? html`<span>${formatSpeed(iface.speed_mbps)}</span>`
              : nothing}
          </p>
        </div>
        <div class="addresses-cell">${this.renderAddresses(iface)}</div>
        <dl class="counters">
          <div>
            <dt>${msg('RX')}</dt>
            <dd>${formatRate(iface.rx_rate)}</dd>
            <dd class="total">${formatBytes(iface.rx_bytes)}</dd>
          </div>
          <div>
            <dt>${msg('TX')}</dt>
            <dd>${formatRate(iface.tx_rate)}</dd>
            <dd class="total">${formatBytes(iface.tx_bytes)}</dd>
          </div>
        </dl>
        ${hasErrors(iface)
          ? html`
              <p class="counter-warning">
                ${msg(
                  str`${iface.rx_errors + iface.tx_errors} errors · ${iface.rx_dropped + iface.tx_dropped} dropped`,
                )}
              </p>
            `
          : nothing}
        ${this.renderActions(iface)}
      </article>
    `
  }

  private renderBody() {
    const visible = this.visibleRows()
    if (!this.snapshot) {
      if (this.error) {
        return html`<p class="error" role="alert">${this.error}</p>`
      }
      if (this.paused) {
        return html`<p class="empty">${msg('Paused — press Resume to read the interfaces again.')}</p>`
      }
      if (this.client) {
        return html`<p class="empty">${msg('Reading the interfaces…')}</p>`
      }
      return html`<p class="empty">${msg('No network information yet.')}</p>`
    }
    return html`
      ${this.error ? html`<p class="error" role="alert">${this.error}</p>` : nothing}
      ${this.actionError
        ? html`<p class="error" role="alert">${this.actionError}</p>`
        : nothing}
      ${this.notice ? html`<p class="notice" role="status">${this.notice}</p>` : nothing}
      ${this.createOpen ? this.renderCreateForm() : nothing}
      ${this.renderRoutes()}
      <div class="filters" part="filters">
        <input
          type="search"
          .value=${this.query}
          placeholder=${msg('Filter by interface, address, MAC or state')}
          aria-label=${msg('Filter network interfaces')}
          @input=${(event: Event) => {
            this.query = (event.currentTarget as HTMLInputElement).value
          }}
        />
        ${visible.length !== this.rows.length
          ? html`<span class="muted"
              >${msg(
                str`${visible.length} of ${this.rows.length} interfaces`,
              )}</span
            >`
          : nothing}
      </div>
      ${visible.length === 0
        ? html`<p class="empty">
            ${this.rows.length === 0
              ? msg('No interfaces found.')
              : msg('No interfaces match the current filter.')}
          </p>`
        : html`<div class="list" part="list">${visible.map((iface) => this.renderCard(iface))}</div>`}
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
    .toolbar .status {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }
    .toolbar .status::before {
      content: '';
      width: 6px;
      height: 6px;
      border-radius: 999px;
      background: currentColor;
    }
    .toolbar .status[data-state='live'] { color: var(--ok, #34d399); }
    .toolbar .status[data-state='error'] { color: var(--err, #f87171); }
    .toolbar .status[data-state='connecting'] { color: var(--accent, #7dd3fc); }
    .toolbar .count { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
    .field { display: inline-flex; align-items: center; gap: 0.35rem;
             margin-left: auto; color: var(--muted, #8b9bad);
             font: 12px/1.4 var(--mono, monospace); }
    .field-label { display: none; }
    .toolbar select,
    .toolbar button {
      min-height: 30px;
      padding: 0.25rem 0.55rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
    }
    .toolbar button { display: inline-flex; align-items: center; gap: 0.35rem; cursor: pointer; }
    .toolbar button:hover,
    .toolbar select:hover { border-color: var(--accent, #7dd3fc); }
    .toolbar button:focus-visible,
    .toolbar select:focus-visible,
    input:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    .toolbar anth-icon { font-size: 14px; }

    .routes {
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
    .routes-label {
      color: var(--muted, #8b9bad);
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
    .route { display: inline-flex; align-items: baseline; gap: 0.35rem; }
    .route .gateway { color: var(--text, #dbe6f2); }
    .route .family,
    .addresses .family {
      color: var(--accent, #7dd3fc);
      font: 600 10px/1.2 var(--mono, monospace);
      letter-spacing: 0.04em;
    }

    .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 0.5rem; }
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

    .list { display: flex; flex-direction: column; gap: 0.5rem; }

    .iface {
      display: grid;
      grid-template-columns: minmax(11rem, 0.9fr) minmax(13rem, 1.1fr) auto;
      align-items: center;
      gap: 0.5rem 1rem;
      padding: 0.65rem 0.75rem;
      border: 1px solid var(--panel-border, #223041);
      border-left: 3px solid var(--muted, #8b9bad);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .iface[data-up='yes'] { border-left-color: var(--ok, #34d399); }
    .iface .title { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem; }
    .iface h3 { margin: 0; font: 600 15px/1.3 var(--mono, monospace); }
    .dot { width: 7px; height: 7px; border-radius: 999px; background: var(--muted, #8b9bad); }
    .iface[data-up='yes'] .dot { background: var(--ok, #34d399); }
    .kind,
    .state {
      padding: 0.1rem 0.35rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }
    .iface[data-up='yes'] .state { color: var(--ok, #34d399); }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: 0.25rem 0.6rem;
      margin: 0.2rem 0 0;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
    }
    .meta .mac { overflow-wrap: anywhere; }

    .addresses-cell { min-width: 0; }
    .addresses { display: flex; flex-wrap: wrap; gap: 0.25rem 0.5rem;
                 margin: 0; padding: 0; list-style: none; }
    .addresses li { display: inline-flex; align-items: baseline; gap: 0.3rem;
                    max-width: 100%; padding: 0.1rem 0.4rem;
                    border: 1px solid var(--panel-border, #223041);
                    border-radius: 6px; background: var(--panel-2, #0b121b);
                    font: 12px/1.45 var(--mono, monospace); }
    .addresses .address { overflow-wrap: anywhere; }
    .addresses .scope { color: var(--muted, #8b9bad); font-size: 10px; }
    .no-address { margin: 0; color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }

    .counters { display: flex; gap: 1rem; margin: 0; }
    .counters div { display: grid; justify-items: end; gap: 0.05rem; }
    .counters dt {
      color: var(--muted, #8b9bad);
      font: 600 10px/1.2 var(--mono, monospace);
      letter-spacing: 0.06em;
    }
    .counters dd { margin: 0; font: 600 13px/1.3 var(--mono, monospace); }
    .counters .total { color: var(--muted, #8b9bad); font-weight: 400; font-size: 11px; }

    .counter-warning {
      grid-column: 1 / -1;
      margin: 0;
      color: var(--warn, #fbbf24);
      font: 11px/1.4 var(--mono, monospace);
    }

    .notice {
      margin: 0;
      padding: 0.55rem 0.65rem;
      border: 1px solid rgba(52, 211, 153, 0.35);
      border-radius: 8px;
      background: rgba(52, 211, 153, 0.08);
      color: var(--ok, #34d399);
      font-size: 13px;
    }

    .create {
      display: flex;
      flex-direction: column;
      gap: 0.6rem;
      padding: 0.65rem 0.75rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .create-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
      gap: 0.5rem 0.75rem;
    }
    .create-grid label,
    .inline label {
      display: grid;
      gap: 0.2rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }
    .create-grid input,
    .create-grid select,
    .inline input {
      min-height: 32px;
      padding: 0.3rem 0.5rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
    }
    .create-grid .check {
      display: flex;
      grid-auto-flow: column;
      align-items: center;
      align-self: end;
      gap: 0.35rem;
      min-height: 32px;
      font-size: 12px;
    }
    .create-grid .check input { min-height: 0; width: auto; }
    .create-actions,
    .actions,
    .confirm,
    .inline {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.4rem;
    }
    .create button,
    .actions button,
    .inline button,
    .confirm button {
      min-height: 30px;
      padding: 0.25rem 0.6rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .create button:hover,
    .actions button:hover,
    .inline button:hover,
    .confirm button:hover { border-color: var(--accent, #7dd3fc); }
    .create button:disabled,
    .actions button:disabled,
    .inline button:disabled,
    .confirm button:disabled { opacity: 0.55; cursor: progress; }
    .actions button.danger,
    .confirm button.danger { color: var(--err, #f87171); }
    .hint { margin: 0; color: var(--muted, #8b9bad); font-size: 11px; }

    .manage {
      grid-column: 1 / -1;
      display: flex;
      flex-direction: column;
      gap: 0.45rem;
      padding-top: 0.5rem;
      border-top: 1px solid var(--panel-border, #223041);
    }
    .actions { justify-content: flex-end; }
    .inline label { grid-auto-flow: column; align-items: center; gap: 0.35rem; }
    .inline input { width: 8rem; }
    .confirm {
      padding: 0.4rem 0.5rem;
      border: 1px solid rgba(248, 113, 113, 0.4);
      border-radius: 7px;
      background: rgba(248, 113, 113, 0.06);
      font-size: 12px;
    }
    .confirm > span:first-of-type { margin-right: auto; }
    .confirm .warning { flex-basis: 100%; color: var(--warn, #fbbf24); }

    .muted { color: var(--muted, #8b9bad); }
    .empty,
    .error {
      margin: 0;
      padding: 0.75rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
    }
    .empty { color: var(--muted, #8b9bad); }
    .error { color: var(--err, #f87171); }

    @media (max-width: 44rem) {
      .iface { grid-template-columns: 1fr; align-items: start; }
      .counters { justify-content: space-between; }
      .counters div { justify-items: start; }
      .field { margin-left: 0; }
      .field-label { display: none; }
      .toolbar select,
      .toolbar button { min-height: 44px; }
      .filters input { min-height: 44px; }
      .actions { justify-content: flex-start; }
      .actions button,
      .inline button,
      .confirm button,
      .create button { min-height: 44px; }
      .create-grid input,
      .create-grid select { min-height: 44px; }
      .inline label { grid-auto-flow: row; }
      .inline input { width: 100%; }
    }
  `
}

function intervalLabel(ms: number): string {
  return ms % 1000 === 0 ? `${ms / 1000}s` : `${ms}ms`
}

function scopeLabel(scope: string): string {
  switch (scope) {
    case 'host':
      return msg('host')
    case 'link':
      return msg('link')
    default:
      return msg('global')
  }
}
