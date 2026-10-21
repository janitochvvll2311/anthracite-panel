import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import type {
  NetworkClient,
  NetworkConnectionStatus,
  NetworkInterface,
  NetworkInterfaceKind,
  NetworkRow,
  NetworkSnapshot,
} from '../lib/network/network-client'
import {
  addressSummary,
  compareNetworkRows,
  computeNetworkRows,
  filterNetworkRows,
  formatAddress,
  formatBytes,
  formatRate,
  formatSpeed,
  interfaceState,
} from '../lib/network/display'
import type {
  NetworkSortDirection,
  NetworkSortKey,
} from '../lib/network/display'
import { confirmAction, promptText, showAlert } from '../lib/ui/dialog'
import { onTabShow } from '../lib/ui/tab-visibility'

export const ANTH_NETWORK_STATE_EVENT = 'anth-network-state'

/** The create form's values, already validated. */
export interface NetworkCreateForm {
  name: string
  kind: NetworkInterfaceKind
  peer?: string
  link?: string
  vlan_id?: number
  mtu?: number
  up?: boolean
}

export interface AnthNetworkStateDetail {
  name: string
  up: boolean
}

const INTERVALS = [1000, 2000, 5000, 10000]

/**
 * Connected network monitor: fetches snapshots, follows the SSE stream, computes
 * per-interface rates and, when the daemon allows it, brings an interface up or
 * down.
 *
 * The component depends on the injected `NetworkClient`, not on `/api`.
 *
 * @fires anth-network-state - `{ name, up }` after a successful state change.
 */
@customElement('anth-network-manager')
export class AnthNetworkManager extends LitElement {
  @property({ attribute: false })
  client?: NetworkClient

  /** SSE interval in milliseconds. */
  @property({ type: Number, attribute: 'refresh-interval' })
  refreshInterval = 2000

  @property({ type: Boolean, attribute: 'auto-start' })
  autoStart = true

  @state()
  private rows: NetworkRow[] = []

  /** Whether the daemon accepts create, edit, rename and delete. */
  @state()
  private manageEnabled = false

  /** Which management form is open, if any. */
  @state()
  private form: 'create' | undefined

  @state()
  private status: NetworkConnectionStatus = 'idle'

  @state()
  private controlEnabled = false

  @state()
  private warning = ''

  @state()
  private errorMessage = ''

  /** A failed up/down action. It survives subsequent snapshots. */
  @state()
  private actionError = ''

  @state()
  private filter = ''

  @state()
  private sortKey: NetworkSortKey = 'name'

  @state()
  private sortDirection: NetworkSortDirection = 'asc'

  @state()
  private selectedName?: string

  @state()
  private busy = false

  private previous?: NetworkSnapshot
  private unsubscribe?: () => void
  private offShow?: () => void
  private resumeOnShow = false
  private generation = 0

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    // Parking must not resurrect a stream the operator paused on purpose, so
    // the decision is taken before `stop()` clears it.
    this.resumeOnShow = this.unsubscribe !== undefined
    this.stop()
    this.offShow?.()
    this.offShow = undefined
  }

  override connectedCallback(): void {
    super.connectedCallback()
    // A tab panel parks a hidden tab with `remove()`, which stops the stream
    // here through `disconnectedCallback`. Coming back changes no property, so
    // `willUpdate` never runs: this listener is the only re-show signal.
    this.offShow = onTabShow(this, () => {
      if (this.resumeOnShow && this.client) this.start()
    })
  }

  protected override willUpdate(changed: PropertyValues<this>): void {
    if (
      (changed.has('client') ||
        changed.has('refreshInterval') ||
        (changed.has('autoStart') && this.autoStart)) &&
      this.client &&
      this.autoStart
    ) {
      this.start()
    }
  }

  /** Starts the initial fetch and the live SSE stream. */
  start(): void {
    this.stop()
    if (!this.client) {
      this.status = 'idle'
      return
    }

    const generation = ++this.generation
    this.previous = undefined
    this.errorMessage = ''
    this.actionError = ''
    this.status = 'connecting'

    this.unsubscribe = this.client.watch(this.refreshInterval, {
      onSnapshot: (snapshot) => {
        if (generation === this.generation) this.applySnapshot(snapshot)
      },
      onError: (error) => {
        if (generation !== this.generation) return
        this.errorMessage = error.message
        if (this.status !== 'live') this.status = 'error'
      },
      onStatus: (status) => {
        if (generation !== this.generation) return
        this.status = status
        if (status === 'live') this.errorMessage = ''
      },
    })

    void this.refreshSnapshot(generation)
  }

  /** Stops the SSE stream. Existing rows stay visible. */
  stop(): void {
    this.generation += 1
    this.unsubscribe?.()
    this.unsubscribe = undefined
    this.previous = undefined
    if (this.status !== 'idle') this.status = 'closed'
  }

  /** Fetches one snapshot immediately, without restarting the stream. */
  refresh(): void {
    void this.refreshSnapshot(this.generation)
  }

  private async refreshSnapshot(generation: number): Promise<void> {
    if (!this.client || this.busy) return

    try {
      const snapshot = await this.client.list()
      if (generation === this.generation) this.applySnapshot(snapshot)
    } catch (error) {
      if (generation !== this.generation) return
      this.errorMessage =
        error instanceof Error ? error.message : String(error)
      this.status = 'error'
    }
  }

  private applySnapshot(snapshot: NetworkSnapshot): void {
    // A state change is in flight; applying an older snapshot now would make
    // the row flip back and forth. The post-action refresh catches up.
    if (this.busy) return

    this.rows = computeNetworkRows(snapshot, this.previous)
    this.previous = snapshot
    this.controlEnabled = snapshot.control_enabled
    this.manageEnabled = snapshot.manage_enabled
    this.warning =
      snapshot.warning ??
      (snapshot.interfaces.length < snapshot.total
        ? `showing ${snapshot.interfaces.length} of ${snapshot.total} interfaces`
        : '')
    this.errorMessage = ''

    if (
      this.selectedName !== undefined &&
      !this.rows.some((iface) => iface.name === this.selectedName)
    ) {
      this.selectedName = undefined
    }
  }

  private get filteredRows(): NetworkRow[] {
    return [...filterNetworkRows(this.rows, this.filter)].sort((left, right) =>
      compareNetworkRows(left, right, this.sortKey, this.sortDirection),
    )
  }

  private get selectedInterface(): NetworkRow | undefined {
    return this.rows.find((iface) => iface.name === this.selectedName)
  }

  private statusLabel(): string {
    return this.status
  }

  private intervals(): number[] {
    return [...new Set([...INTERVALS, this.refreshInterval])].sort(
      (a, b) => a - b,
    )
  }

  private onFilterInput(event: Event): void {
    this.filter = (event.target as HTMLInputElement).value
  }

  private onIntervalChange(event: Event): void {
    this.refreshInterval = Number(
      (event.currentTarget as HTMLSelectElement).value,
    )
  }

  private sortBy(key: NetworkSortKey): void {
    if (this.sortKey === key) {
      this.sortDirection = this.sortDirection === 'asc' ? 'desc' : 'asc'
      return
    }

    this.sortKey = key
    this.sortDirection = key === 'name' || key === 'state' ? 'asc' : 'desc'
  }

  private select(name: string): void {
    this.selectedName = name
  }

  private onRowKeydown(event: KeyboardEvent, name: string): void {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    this.select(name)
  }

  private canChange(iface: NetworkInterface): boolean {
    return this.controlEnabled && !(iface.loopback && iface.up)
  }

  private async changeState(
    event: Event,
    iface: NetworkInterface,
    up: boolean,
  ): Promise<void> {
    event.stopPropagation()
    if (!this.client || !this.canChange(iface) || this.busy) return

    const accepted = await confirmAction({
      title: up ? `Bring up ${iface.name}?` : `Bring down ${iface.name}?`,
      message: up
        ? 'The interface will be enabled.'
        : 'This can disconnect the host.',
      confirmLabel: up ? 'Bring up' : 'Bring down',
      danger: !up,
    })
    if (!accepted) return

    this.busy = true
    this.actionError = ''
    let changed = false
    try {
      await this.client.setState(iface.name, up)
      changed = true
      this.dispatchEvent(
        new CustomEvent<AnthNetworkStateDetail>(ANTH_NETWORK_STATE_EVENT, {
          detail: { name: iface.name, up },
          bubbles: true,
          composed: true,
        }),
      )
    } catch (error) {
      this.actionError =
        error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
    }

    if (changed) this.refresh()
  }

  /**
   * Whether a management action applies to an interface.
   *
   * This mirrors the daemon's guards so the UI disables a row instead of
   * offering an action certain to fail. It is a hint, never the policy: the
   * daemon re-checks everything, including the peer-address guard, which the
   * UI cannot evaluate.
   */
  manageBlockReason(iface: NetworkInterface): string {
    if (!this.manageEnabled) return 'network management is disabled'
    if (iface.kind === 'loopback') return 'loopback interfaces are protected'
    return ''
  }

  renameBlockReason(iface: NetworkInterface): string {
    const base = this.manageBlockReason(iface)
    if (base !== '') return base
    if (iface.up) return 'bring the interface down first'
    return ''
  }

  private async createInterface(request: NetworkCreateForm): Promise<void> {
    if (!this.client || this.busy) return
    this.busy = true
    this.actionError = ''
    try {
      await this.client.create(request)
      this.form = undefined
    } catch (error) {
      this.actionError =
        error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
      this.refresh()
    }
  }

  private async editInterface(
    iface: NetworkRow,
    edit: { mtu?: number; up?: boolean },
  ): Promise<void> {
    if (!this.client || this.busy) return
    this.busy = true
    this.actionError = ''
    try {
      await this.client.edit(iface.name, edit)
      this.form = undefined
    } catch (error) {
      this.actionError =
        error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
      this.refresh()
    }
  }

  private async renameInterface(iface: NetworkRow): Promise<void> {
    if (!this.client || this.busy) return
    if (this.renameBlockReason(iface) !== '') return

    const newName = await promptText({
      title: `Rename ${iface.name}`,
      message: 'The interface must be down. Traffic stops until it is up again.',
      label: 'New name',
      value: iface.name,
      confirmLabel: 'Rename',
      validate: (value) => {
        const trimmed = value.trim()
        if (trimmed === '') return 'A name is required'
        if (trimmed === iface.name) return 'The name did not change'
        if (trimmed.length > 15) return 'Names are limited to 15 bytes'
        if (/[\s/:]/.test(trimmed))
          return 'Names cannot contain spaces, colons or slashes'
        return undefined
      },
    })
    if (newName === undefined) return

    this.busy = true
    this.actionError = ''
    try {
      await this.client.rename(iface.name, newName.trim())
    } catch (error) {
      this.actionError =
        error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
      this.refresh()
    }
  }

  private async deleteInterface(iface: NetworkRow): Promise<void> {
    if (!this.client || this.busy) return

    const blocked = this.manageBlockReason(iface)
    if (blocked !== '') {
      await showAlert({ title: 'Cannot delete this interface', message: blocked })
      return
    }

    // A physical device gets an explicit warning rather than a hidden action:
    // removing a card may be exactly what the operator wants, but they have to
    // know the host loses that link.
    const warning = iface.physical
      ? ` ${iface.name} is a physical device. The host will lose this link, and any service using it will go down with it.`
      : ''

    const confirmed = await confirmAction({
      title: `Delete ${iface.name}?`,
      message: `This cannot be undone.${warning}`,
      confirmLabel: 'Delete',
      danger: true,
    })
    if (!confirmed) return

    this.busy = true
    this.actionError = ''
    try {
      await this.client.remove(iface.name)
    } catch (error) {
      this.actionError =
        error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
      this.refresh()
    }
  }

  private renderSortButton(key: NetworkSortKey, label: string) {
    const active = this.sortKey === key
    const indicator = active ? (this.sortDirection === 'asc' ? '▲' : '▼') : ''

    return html`
      <button
        type="button"
        class="sort ${active ? 'active' : ''}"
        aria-label=${`Sort by ${label.toLowerCase()}`}
        @click=${() => this.sortBy(key)}
      >
        <span>${label}</span>
        <span class="indicator" aria-hidden="true">${indicator}</span>
      </button>
    `
  }

  private renderState(iface: NetworkRow) {
    const state = interfaceState(iface)
    return html`<span class="state" data-tone=${state.tone}>${state.label}</span>`
  }

  private renderAction(iface: NetworkRow) {
    if (!this.controlEnabled) return nothing
    if (iface.loopback && iface.up) {
      return html`<span class="muted" title="Loopback cannot be brought down"
        >protected</span
      >`
    }

    const up = !iface.up
    return html`
      <button
        type="button"
        class=${up ? 'action up' : 'action danger'}
        ?disabled=${this.busy}
        @click=${(event: Event) => this.changeState(event, iface, up)}
      >
        ${up ? 'Up' : 'Down'}
      </button>
    `
  }

  private renderAddresses(iface: NetworkRow) {
    const title = iface.addresses.map(formatAddress).join(', ')
    return html`
      <span class="address" title=${title || iface.mac}>
        ${addressSummary(iface)}
      </span>
    `
  }

  private renderDetail() {
    const iface = this.selectedInterface
    if (!iface) {
      return html`<span class="muted"
        >Select an interface to inspect its addresses and counters.</span
      >`
    }

    return html`
      <div class="detail-name">
        <strong>${iface.name}</strong>
        ${this.renderState(iface)}
      </div>
      <dl class="detail-grid">
        <div><dt>MAC</dt><dd>${iface.mac || '—'}</dd></div>
        <div><dt>MTU</dt><dd>${iface.mtu}</dd></div>
        <div><dt>Speed</dt><dd>${formatSpeed(iface.speed_mbps)}</dd></div>
        <div><dt>Duplex</dt><dd>${iface.duplex ?? '—'}</dd></div>
        <div><dt>Operstate</dt><dd>${iface.state}</dd></div>
        <div>
          <dt>Packets RX / TX</dt>
          <dd>${iface.rx_packets} / ${iface.tx_packets}</dd>
        </div>
        <div>
          <dt>Errors RX / TX</dt>
          <dd>${iface.rx_errors} / ${iface.tx_errors}</dd>
        </div>
        <div>
          <dt>Dropped RX / TX</dt>
          <dd>${iface.rx_dropped} / ${iface.tx_dropped}</dd>
        </div>
      </dl>
      <div class="address-list">
        ${iface.addresses.length === 0
          ? html`<span class="muted">No IP addresses assigned.</span>`
          : iface.addresses.map(
              (address) => html`
                <span class="chip" data-family=${address.family}>
                  ${formatAddress(address)}
                </span>
              `,
            )}
      </div>
      ${this.renderDetailActions(iface)}
    `
  }

  /**
   * Edit, rename and delete for the selected interface.
   *
   * Every control carries its reason for being disabled in a `title`, so an
   * operator sees why an action is unavailable instead of clicking a dead
   * button.
   */
  private renderDetailActions(iface: NetworkRow) {
    if (!this.manageEnabled) return nothing

    const deleteBlocked = this.manageBlockReason(iface)
    const renameBlocked = this.renameBlockReason(iface)
    const kind = iface.kind === 'other' ? 'interface' : iface.kind

    return html`
      <div class="detail-actions">
        <span class="muted kind">${kind}${iface.physical ? ' · physical' : ''}</span>
        <span class="spacer"></span>
        <button
          type="button"
          ?disabled=${this.busy}
          @click=${() => this.editMtu(iface)}
        >
          Edit MTU
        </button>
        <button
          type="button"
          ?disabled=${this.busy || renameBlocked !== ''}
          title=${renameBlocked}
          @click=${() => this.renameInterface(iface)}
        >
          Rename
        </button>
        <button
          type="button"
          class="danger"
          ?disabled=${this.busy || deleteBlocked !== ''}
          title=${deleteBlocked}
          @click=${() => this.deleteInterface(iface)}
        >
          Delete
        </button>
      </div>
    `
  }

  private toggleCreateForm(): void {
    this.form = this.form === 'create' ? undefined : 'create'
  }

  private renderCreateForm() {
    return html`
      <form class="create" part="create" @submit=${this.onCreateSubmit}>
        <label class="field">
          Name
          <input
            name="name"
            required
            maxlength="15"
            placeholder="br0"
            ?disabled=${this.busy}
          />
        </label>
        <label class="field">
          Kind
          <select name="kind" ?disabled=${this.busy}>
            ${(
              [
                ['bridge', 'bridge'],
                ['bond', 'bond'],
                ['dummy', 'dummy'],
                ['veth', 'veth'],
                ['vlan', 'vlan'],
                ['tun_tap', 'tun/tap'],
              ] as const
            ).map(
              ([value, label]) =>
                html`<option value=${value}>${label}</option>`,
            )}
          </select>
        </label>
        <label class="field">
          MTU
          <input
            name="mtu"
            type="number"
            min="68"
            max="65535"
            placeholder="1500"
            ?disabled=${this.busy}
          />
        </label>
        <label class="field">
          Peer / parent
          <input
            name="link"
            maxlength="15"
            placeholder="for veth or vlan"
            ?disabled=${this.busy}
          />
        </label>
        <label class="field">
          VLAN id
          <input
            name="vlan_id"
            type="number"
            min="1"
            max="4094"
            ?disabled=${this.busy}
          />
        </label>
        <label class="field checkbox">
          <input type="checkbox" name="up" ?disabled=${this.busy} />
          Bring up
        </label>
        <span class="spacer"></span>
        <button type="submit" ?disabled=${this.busy}>Create</button>
        <button type="button" @click=${this.toggleCreateForm} ?disabled=${this.busy}>
          Cancel
        </button>
      </form>
      <p class="hint muted">
        Only virtual interfaces can be created. Addresses are not assigned here.
      </p>
    `
  }

  private onCreateSubmit = (event: SubmitEvent): void => {
    event.preventDefault()
    const form = event.target as HTMLFormElement
    const data = new FormData(form)
    const name = String(data.get('name') ?? '').trim()
    if (name === '') return

    const kind = String(data.get('kind') ?? 'bridge') as NetworkInterfaceKind
    const mtuText = String(data.get('mtu') ?? '').trim()
    const linkText = String(data.get('link') ?? '').trim()
    const vlanText = String(data.get('vlan_id') ?? '').trim()

    const request: NetworkCreateForm = {
      name,
      kind,
      up: data.get('up') === 'on',
    }
    if (mtuText !== '') request.mtu = Number(mtuText)
    if (linkText !== '') {
      // A veth calls it a peer, a vlan calls it a parent; the daemon wants the
      // right field for each.
      if (kind === 'veth') request.peer = linkText
      else request.link = linkText
    }
    if (vlanText !== '') request.vlan_id = Number(vlanText)

    void this.createInterface(request)
  }

  /** Asks for a new MTU, or returns `undefined` when the operator cancels. */
  private async promptMtu(iface: NetworkRow): Promise<number | undefined> {
    const raw = await promptText({
      title: `MTU for ${iface.name}`,
      label: 'MTU',
      value: String(iface.mtu),
      confirmLabel: 'Apply',
      validate: (value) => {
        const parsed = Number(value.trim())
        if (!Number.isInteger(parsed)) return 'The MTU must be a whole number'
        if (parsed < 68 || parsed > 65535) return 'The MTU must be 68-65535'
        return undefined
      },
    })
    return raw === undefined ? undefined : Number(raw.trim())
  }

  private async editMtu(iface: NetworkRow): Promise<void> {
    const mtu = await this.promptMtu(iface)
    if (mtu === undefined) return
    await this.editInterface(iface, { mtu })
  }

  override render() {
    const up = this.rows.filter((iface) => iface.up).length

    return html`
      <section class="monitor" part="monitor" aria-busy=${this.busy}>
        <header class="toolbar" part="toolbar">
          <span class="status" part="status" data-status=${this.status}>
            <span class="dot" aria-hidden="true"></span>
            ${this.statusLabel()}
          </span>
          <span class="count">
            ${this.rows.length} interface${this.rows.length === 1 ? '' : 's'} ·
            ${up} up
          </span>
          <span class="spacer"></span>
          <label class="field">
            Interval
            <select
              aria-label="Refresh interval"
              .value=${String(this.refreshInterval)}
              @change=${this.onIntervalChange}
            >
              ${this.intervals().map(
                (interval) => html`
                  <option value=${String(interval)}>${interval / 1000}s</option>
                `,
              )}
            </select>
          </label>
          <button
            type="button"
            @click=${this.toggleCreateForm}
            ?disabled=${!this.client || !this.manageEnabled || this.busy}
            title=${this.manageEnabled
              ? 'Create a virtual interface'
              : 'network management is disabled'}
          >
            Create
          </button>
          <button
            type="button"
            @click=${this.refresh}
            ?disabled=${!this.client || this.busy}
          >
            Refresh
          </button>
          <button
            type="button"
            @click=${this.status === 'live' || this.status === 'connecting'
              ? this.stop
              : this.start}
            ?disabled=${!this.client}
          >
            ${this.status === 'live' || this.status === 'connecting'
              ? 'Pause'
              : 'Resume'}
          </button>
        </header>

        ${this.form === 'create' ? this.renderCreateForm() : nothing}
        ${this.actionError === ''
          ? nothing
          : html`
              <p class="error" part="action-error" role="alert">
                ${this.actionError}
              </p>
            `}
        ${this.errorMessage === ''
          ? nothing
          : html`
              <p class="error" part="error" role="alert">${this.errorMessage}</p>
            `}
        ${this.warning === ''
          ? nothing
          : html`<p class="warning" part="warning">${this.warning}</p>`}

        <div class="filters" part="filters">
          <input
            type="search"
            .value=${this.filter}
            placeholder="Filter by interface, address, MAC or state"
            aria-label="Filter network interfaces"
            @input=${this.onFilterInput}
          />
        </div>

        <div class="table-wrap" part="table">
          <table>
            <thead>
              <tr>
                <th aria-sort=${this.sortKey === 'name' ? this.sortDirection : 'none'}>
                  ${this.renderSortButton('name', 'Interface')}
                </th>
                <th aria-sort=${this.sortKey === 'state' ? this.sortDirection : 'none'}>
                  ${this.renderSortButton('state', 'State')}
                </th>
                <th>Address</th>
                <th class="number" aria-sort=${this.sortKey === 'speed' ? this.sortDirection : 'none'}>
                  ${this.renderSortButton('speed', 'Speed')}
                </th>
                <th class="number" aria-sort=${this.sortKey === 'rx_rate' ? this.sortDirection : 'none'}>
                  ${this.renderSortButton('rx_rate', 'RX/s')}
                </th>
                <th class="number" aria-sort=${this.sortKey === 'tx_rate' ? this.sortDirection : 'none'}>
                  ${this.renderSortButton('tx_rate', 'TX/s')}
                </th>
                <th class="number" aria-sort=${this.sortKey === 'rx_bytes' ? this.sortDirection : 'none'}>
                  ${this.renderSortButton('rx_bytes', 'RX')}
                </th>
                <th class="number" aria-sort=${this.sortKey === 'tx_bytes' ? this.sortDirection : 'none'}>
                  ${this.renderSortButton('tx_bytes', 'TX')}
                </th>
                <th class="actions-head">Control</th>
              </tr>
            </thead>
            <tbody>
              ${this.filteredRows.map(
                (iface) => html`
                  <tr
                    class=${iface.name === this.selectedName ? 'selected' : ''}
                    tabindex="0"
                    @click=${() => this.select(iface.name)}
                    @keydown=${(event: KeyboardEvent) =>
                      this.onRowKeydown(event, iface.name)}
                  >
                    <td>
                      <span class="name">
                        <span
                          class="link-dot"
                          data-tone=${interfaceState(iface).tone}
                          aria-hidden="true"
                        ></span>
                        ${iface.name}
                      </span>
                    </td>
                    <td>${this.renderState(iface)}</td>
                    <td>${this.renderAddresses(iface)}</td>
                    <td class="number">${formatSpeed(iface.speed_mbps)}</td>
                    <td class="number">${formatRate(iface.rx_rate)}</td>
                    <td class="number">${formatRate(iface.tx_rate)}</td>
                    <td class="number">${formatBytes(iface.rx_bytes)}</td>
                    <td class="number">${formatBytes(iface.tx_bytes)}</td>
                    <td class="actions">${this.renderAction(iface)}</td>
                  </tr>
                `,
              )}
            </tbody>
          </table>
          ${this.filteredRows.length === 0
            ? html`<p class="empty">
                ${this.client
                  ? 'No interfaces match the current filter.'
                  : 'No network client configured.'}
              </p>`
            : nothing}
        </div>

        <footer class="details" part="details">
          ${this.renderDetail()}
          ${this.controlEnabled
            ? nothing
            : html`<span class="muted control-off"
                >Control is disabled by the daemon.</span
              >`}
        </footer>
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

    .monitor {
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
    .filters {
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

    .count {
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .status {
      gap: 6px;
      padding: 2px 8px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
      text-transform: lowercase;
    }

    .dot,
    .link-dot {
      width: 7px;
      height: 7px;
      border-radius: 50%;
      background: var(--muted, #8b9bad);
    }

    .status[data-status='connecting'] .dot {
      background: #fbbf24;
    }

    .status[data-status='live'] .dot {
      background: #34d399;
    }

    .status[data-status='error'] .dot {
      background: #f87171;
    }

    .link-dot[data-tone='up'] {
      background: #34d399;
    }

    .link-dot[data-tone='down'] {
      background: #64748b;
    }

    .link-dot[data-tone='warn'] {
      background: #fbbf24;
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
    tr:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .field {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .filters input {
      width: min(32rem, 100%);
    }

    .error {
      margin: 0;
      padding: 8px 10px;
      border-radius: 6px;
      background: rgba(248, 113, 113, 0.12);
      color: #fecaca;
    }

    .warning {
      margin: 0;
      padding: 6px 10px;
      border-radius: 6px;
      background: rgba(251, 191, 36, 0.1);
      color: #fcd34d;
      font: 12px/1.4 var(--mono, monospace);
    }

    .table-wrap {
      max-height: var(--network-table-height, 30rem);
      overflow: auto;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      scrollbar-color: var(--panel-border, #223041) transparent;
      scrollbar-width: thin;
    }

    table {
      width: 100%;
      min-width: 72rem;
      table-layout: fixed;
      border-collapse: collapse;
      white-space: nowrap;
      font: 13px/1.3 var(--mono, monospace);
    }

    thead {
      position: sticky;
      top: 0;
      z-index: 1;
      background: #131a23;
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
      width: 9.5rem;
    }

    th:nth-child(2),
    td:nth-child(2) {
      width: 6.5rem;
    }

    th:nth-child(3),
    td:nth-child(3) {
      width: 16rem;
    }

    th:nth-child(4),
    td:nth-child(4) {
      width: 6.5rem;
    }

    th:nth-child(5),
    td:nth-child(5),
    th:nth-child(6),
    td:nth-child(6) {
      width: 6.5rem;
    }

    th:nth-child(7),
    td:nth-child(7),
    th:nth-child(8),
    td:nth-child(8) {
      width: 7.5rem;
    }

    th:nth-child(9),
    td:nth-child(9) {
      width: 5.5rem;
    }

    th {
      color: var(--muted, #8b9bad);
      font-size: 0.78em;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }

    .sort {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.35rem;
      width: 100%;
      padding: 0;
      border: 0;
      background: transparent;
      color: inherit;
      font: inherit;
      text-align: left;
      text-transform: uppercase;
    }

    th.number .sort {
      justify-content: flex-end;
    }

    .indicator {
      color: var(--accent, #7dd3fc);
      font-size: 0.75em;
    }

    tbody tr {
      cursor: default;
    }

    tbody tr:hover {
      background: rgba(125, 211, 252, 0.06);
    }

    tbody tr.selected {
      background: rgba(125, 211, 252, 0.14);
    }

    tbody tr:last-child td {
      border-bottom: 0;
    }

    .name {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      font-weight: 600;
    }

    .address {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .number {
      text-align: right;
      font-variant-numeric: tabular-nums;
    }

    .state {
      display: inline-flex;
      align-items: center;
      padding: 1px 6px;
      border-radius: 999px;
      font-size: 0.82em;
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }

    .state[data-tone='up'] {
      background: rgba(52, 211, 153, 0.14);
      color: #6ee7b7;
    }

    .state[data-tone='down'] {
      background: rgba(100, 116, 139, 0.2);
      color: #94a3b8;
    }

    .state[data-tone='warn'] {
      background: rgba(251, 191, 36, 0.14);
      color: #fcd34d;
    }

    .actions {
      text-align: right;
    }

    .actions-head {
      text-align: right;
    }

    .action {
      padding: 2px 7px;
      font-size: 0.85em;
    }

    .action.up {
      color: #6ee7b7;
    }

    .action.danger {
      color: #fca5a5;
    }

    .details {
      display: flex;
      flex-wrap: wrap;
      align-items: flex-start;
      justify-content: space-between;
      gap: 12px;
      min-height: 3.5rem;
      padding: 9px 11px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: #0c131c;
    }

    .detail-name {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
    }

    .create {
      display: flex;
      flex-wrap: wrap;
      align-items: flex-end;
      gap: 0.6rem;
      padding: 0.75rem;
      border-bottom: 1px solid var(--border, #24303f);
      background: var(--surface-2, #131a24);
    }

    .create input {
      width: 8rem;
    }

    .create .checkbox {
      flex-direction: row;
      align-items: center;
      gap: 0.35rem;
    }

    .create .checkbox input {
      width: auto;
    }

    .hint {
      margin: 0;
      padding: 0.4rem 0.75rem;
      font-size: 12px;
    }

    .detail-actions {
      display: flex;
      align-items: center;
      gap: 0.4rem;
      margin-top: 0.6rem;
      padding-top: 0.6rem;
      border-top: 1px solid var(--border, #24303f);
    }

    .detail-actions .kind {
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }

    button.danger {
      border-color: var(--danger, #b4232a);
      color: var(--danger-text, #ff9a9f);
    }

    .detail-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
      flex: 1;
      gap: 4px 14px;
      margin: 0;
    }

    .detail-grid div {
      display: flex;
      gap: 8px;
      justify-content: space-between;
      min-width: 0;
    }

    dt {
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    dd {
      margin: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      font: 12px/1.4 var(--mono, monospace);
    }

    .address-list {
      display: flex;
      flex-wrap: wrap;
      gap: 5px;
      width: 100%;
    }

    .chip {
      padding: 1px 7px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font: 11px/1.5 var(--mono, monospace);
    }

    .chip[data-family='ipv4'] {
      color: #7dd3fc;
    }

    .chip[data-family='ipv6'] {
      color: #c4b5fd;
    }

    .muted,
    .empty {
      color: var(--muted, #8b9bad);
    }

    .empty {
      margin: 0;
      padding: 18px 10px;
      text-align: center;
    }

    .control-off {
      font: 12px/1.4 var(--mono, monospace);
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-network-manager': AnthNetworkManager
  }

  interface HTMLElementEventMap {
    'anth-network-state': CustomEvent<AnthNetworkStateDetail>
  }
}
