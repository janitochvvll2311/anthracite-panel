import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import type {
  VolumeClient,
  VolumeDevice,
  VolumeMountpoint,
  VolumeSnapshot,
} from '../lib/volumes/volume-client'
import {
  availableLabel,
  compareDeviceRows,
  compareMounts,
  deviceCountLabel,
  filterDeviceRows,
  filterMounts,
  kindLabel,
  mountCountLabel,
  mountMode,
  mountableDevices,
  toDeviceRows,
  usageLabel,
  usedPercent,
} from '../lib/volumes/display'
import type {
  DeviceRow,
  VolumeMountSortKey,
  VolumeSortKey,
} from '../lib/volumes/display'
import { formatBytes } from '../lib/format'

/**
 * Storage volume manager.
 *
 * It depends on the injected `VolumeClient`; the HTTP implementation lives in
 * `lib/volumes/volume-client.ts`. Refresh is manual by design, so the component
 * never opens a stream of its own.
 */
@customElement('anth-volume-manager')
export class AnthVolumeManager extends LitElement {
  @property({ attribute: false })
  client?: VolumeClient

  @property({ type: Boolean, attribute: 'auto-load' })
  autoLoad = true

  @state()
  private snapshot?: VolumeSnapshot

  @state()
  private loading = false

  @state()
  private busy = false

  @state()
  private errorMessage = ''

  @state()
  private notice = ''

  @state()
  private filter = ''

  @state()
  private deviceSort: VolumeSortKey = 'device'

  @state()
  private deviceDirection: 'asc' | 'desc' = 'asc'

  @state()
  private mountSort: VolumeMountSortKey = 'mountpoint'

  @state()
  private mountDirection: 'asc' | 'desc' = 'asc'

  @state()
  private mountDevice = ''

  @state()
  private mountTarget = ''

  @state()
  private mountType = ''

  @state()
  private mountReadOnly = false

  @state()
  private mountAllowRemovable = false

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
    // A refresh during a mutation would race the state it is changing.
    if (!this.client || this.loading || this.busy) return

    this.loading = true
    this.errorMessage = ''
    try {
      this.snapshot = await this.client.get()
      this.syncSelection()
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.loading = false
    }
  }

  private get canControl(): boolean {
    return this.snapshot?.control_enabled === true
  }

  private get devices(): DeviceRow[] {
    return toDeviceRows(this.snapshot?.devices ?? [])
  }

  private get visibleDevices(): DeviceRow[] {
    return filterDeviceRows(this.devices, this.filter).sort((left, right) =>
      compareDeviceRows(left, right, this.deviceSort, this.deviceDirection),
    )
  }

  private get visibleMounts(): VolumeMountpoint[] {
    return filterMounts(this.snapshot?.mounts ?? [], this.filter).sort(
      (left, right) =>
        compareMounts(left, right, this.mountSort, this.mountDirection),
    )
  }

  private get candidates(): VolumeDevice[] {
    return mountableDevices(this.snapshot?.devices ?? [])
  }

  /** Keeps the device select pointing at something that still exists. */
  private syncSelection(): void {
    const candidates = this.candidates
    if (candidates.some((device) => device.path === this.mountDevice)) return
    this.mountDevice = candidates[0]?.path ?? ''
    this.mountType = candidates[0]?.fstype ?? ''
  }

  private onFilterInput(event: Event): void {
    this.filter = (event.target as HTMLInputElement).value
  }

  private onDeviceSelect(event: Event): void {
    this.mountDevice = (event.target as HTMLSelectElement).value
    const device = this.candidates.find((entry) => entry.path === this.mountDevice)
    this.mountType = device?.fstype ?? ''
    this.mountAllowRemovable = false
  }

  private onTargetInput(event: Event): void {
    this.mountTarget = (event.target as HTMLInputElement).value
  }

  private onTypeInput(event: Event): void {
    this.mountType = (event.target as HTMLInputElement).value
  }

  private onReadOnlyChange(event: Event): void {
    this.mountReadOnly = (event.target as HTMLInputElement).checked
  }

  private onAllowRemovableChange(event: Event): void {
    this.mountAllowRemovable = (event.target as HTMLInputElement).checked
  }

  private selectedDevice(): VolumeDevice | undefined {
    return this.candidates.find((device) => device.path === this.mountDevice)
  }

  private async mount(event: Event): Promise<void> {
    event.preventDefault()
    if (!this.client || !this.canControl || this.busy) return

    const target = this.mountTarget.trim()
    if (target === '') {
      this.errorMessage = 'A target directory is required.'
      return
    }
    if (!target.startsWith('/')) {
      this.errorMessage = 'The target must be an absolute path.'
      return
    }

    const device = this.selectedDevice()
    if (device?.removable === true && !this.mountAllowRemovable) {
      this.errorMessage =
        'This device is removable. Tick the removable option to mount it anyway.'
      return
    }

    this.busy = true
    this.errorMessage = ''
    this.notice = ''
    let changed = false
    try {
      const type = this.mountType.trim()
      this.notice = await this.client.mount({
        device: this.mountDevice,
        target,
        fs_type: type === '' ? undefined : type,
        read_only: this.mountReadOnly,
        allow_removable: this.mountAllowRemovable,
      })
      this.mountTarget = ''
      this.mountReadOnly = false
      this.mountAllowRemovable = false
      changed = true
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.busy = false
    }

    if (changed) await this.refresh()
  }

  private async unmount(mount: VolumeMountpoint): Promise<void> {
    if (!this.client || !this.canControl || this.busy) return
    if (!window.confirm(`Unmount ${mount.mountpoint}?`)) return

    this.busy = true
    this.errorMessage = ''
    this.notice = ''
    let changed = false
    try {
      this.notice = await this.client.unmount(mount.mountpoint)
      changed = true
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.busy = false
    }

    if (changed) await this.refresh()
  }

  private sortDevices(key: VolumeSortKey): void {
    if (this.deviceSort === key) {
      this.deviceDirection = this.deviceDirection === 'asc' ? 'desc' : 'asc'
      return
    }
    this.deviceSort = key
    this.deviceDirection = key === 'size' ? 'desc' : 'asc'
  }

  private sortMounts(key: VolumeMountSortKey): void {
    if (this.mountSort === key) {
      this.mountDirection = this.mountDirection === 'asc' ? 'desc' : 'asc'
      return
    }
    this.mountSort = key
    this.mountDirection = key === 'used' || key === 'available' ? 'desc' : 'asc'
  }

  private deviceSortMark(key: VolumeSortKey): string {
    if (this.deviceSort !== key) return ''
    return this.deviceDirection === 'asc' ? ' ▲' : ' ▼'
  }

  private mountSortMark(key: VolumeMountSortKey): string {
    if (this.mountSort !== key) return ''
    return this.mountDirection === 'asc' ? ' ▲' : ' ▼'
  }

  private renderForm() {
    const candidates = this.candidates
    const device = this.selectedDevice()

    return html`
      <div class="form-wrap">
        <form class="mount-form" part="form" @submit=${this.mount}>
          <select
            aria-label="Device"
            .value=${this.mountDevice}
            ?disabled=${candidates.length === 0}
            @change=${this.onDeviceSelect}
          >
            ${candidates.length === 0
              ? html`<option value="">No mountable devices</option>`
              : candidates.map(
                  (entry) => html`
                    <option value=${entry.path}>
                      ${entry.name} — ${entry.fstype ?? 'unknown'}${entry.removable
                        ? ' (removable)'
                        : ''}
                    </option>
                  `,
                )}
          </select>
          <input
            type="text"
            placeholder="Target directory, e.g. /mnt/data"
            aria-label="Target directory"
            .value=${this.mountTarget}
            @input=${this.onTargetInput}
          />
          <input
            type="text"
            class="type"
            placeholder="Filesystem"
            aria-label="Filesystem type"
            .value=${this.mountType}
            @input=${this.onTypeInput}
          />
          <label class="check">
            <input
              type="checkbox"
              .checked=${this.mountReadOnly}
              @change=${this.onReadOnlyChange}
            />
            Read-only
          </label>
          ${device?.removable
            ? html`
                <label class="check">
                  <input
                    type="checkbox"
                    .checked=${this.mountAllowRemovable}
                    @change=${this.onAllowRemovableChange}
                  />
                  Allow removable
                </label>
              `
            : nothing}
          <button
            type="submit"
            ?disabled=${this.busy || candidates.length === 0}
          >
            Mount
          </button>
        </form>
        <p class="hint">
          Targets must already exist and stay inside the daemon's mount root. A
          filesystem type is detected from the device when left blank.
        </p>
      </div>
    `
  }

  private renderDevices() {
    const rows = this.visibleDevices
    if (rows.length === 0) {
      return html`<p class="empty">
        ${this.snapshot === undefined
          ? this.loading
            ? 'Loading volumes…'
            : 'No volume snapshot yet.'
          : 'No devices match the filter.'}
      </p>`
    }

    return html`
      <div class="table-wrap">
        <table>
          <caption>
            Block devices
            <span class="muted">${deviceCountLabel(this.snapshot!)}</span>
          </caption>
          <thead>
            <tr>
              <th>
                <button type="button" class="sort" @click=${() => this.sortDevices('device')}>
                  Device${this.deviceSortMark('device')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortDevices('size')}>
                  Size${this.deviceSortMark('size')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortDevices('type')}>
                  Type${this.deviceSortMark('type')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortDevices('fstype')}>
                  Filesystem${this.deviceSortMark('fstype')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortDevices('label')}>
                  Label${this.deviceSortMark('label')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortDevices('mount')}>
                  Mounted on${this.deviceSortMark('mount')}
                </button>
              </th>
            </tr>
          </thead>
          <tbody>
            ${rows.map(
              (row) => html`
                <tr>
                  <td class="mono device" style="--depth: ${row.depth}">
                    <span class="name" title=${row.path}>${row.name}</span>
                    ${row.removable
                      ? html`<span class="tag" title="Removable media">usb</span>`
                      : nothing}
                    ${row.read_only
                      ? html`<span class="tag" title="Read-only device">ro</span>`
                      : nothing}
                  </td>
                  <td class="mono">${formatBytes(row.size_bytes)}</td>
                  <td class="mono">${kindLabel(row.kind)}</td>
                  <td class="mono">${row.fstype ?? '—'}</td>
                  <td>${row.label ?? '—'}</td>
                  <td class="mono" title=${row.mountLabel}>${row.mountLabel}</td>
                </tr>
              `,
            )}
          </tbody>
        </table>
      </div>
    `
  }

  private renderMounts() {
    const mounts = this.visibleMounts
    if (mounts.length === 0) {
      return html`<p class="empty">
        ${this.snapshot === undefined
          ? 'No mounts yet.'
          : 'No mounts match the filter.'}
      </p>`
    }

    return html`
      <div class="table-wrap">
        <table>
          <caption>
            Mounts
            <span class="muted">${mountCountLabel(this.snapshot!)}</span>
          </caption>
          <thead>
            <tr>
              <th>
                <button type="button" class="sort" @click=${() => this.sortMounts('mountpoint')}>
                  Mountpoint${this.mountSortMark('mountpoint')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortMounts('source')}>
                  Source${this.mountSortMark('source')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortMounts('fstype')}>
                  Filesystem${this.mountSortMark('fstype')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortMounts('used')}>
                  Used${this.mountSortMark('used')}
                </button>
              </th>
              <th>
                <button type="button" class="sort" @click=${() => this.sortMounts('available')}>
                  Free${this.mountSortMark('available')}
                </button>
              </th>
              <th>Mode</th>
              <th class="actions-head">Control</th>
            </tr>
          </thead>
          <tbody>
            ${mounts.map((mount) => {
              const percent = usedPercent(mount)
              return html`
                <tr>
                  <td class="mono" title=${mount.mountpoint}>${mount.mountpoint}</td>
                  <td class="mono" title=${mount.source}>${mount.source}</td>
                  <td class="mono">${mount.fstype}</td>
                  <td class="mono">
                    ${usageLabel(mount)}
                    ${percent === null
                      ? nothing
                      : html`<span
                          class="bar"
                          style="--percent: ${percent.toFixed(1)}"
                          aria-hidden="true"
                        ></span>`}
                  </td>
                  <td class="mono">${availableLabel(mount)}</td>
                  <td class="mono">${mountMode(mount)}</td>
                  <td class="actions">
                    ${this.canControl && !mount.virtual_fs
                      ? html`
                          <button
                            type="button"
                            class="danger"
                            ?disabled=${this.busy}
                            @click=${() => this.unmount(mount)}
                          >
                            Unmount
                          </button>
                        `
                      : nothing}
                  </td>
                </tr>
              `
            })}
          </tbody>
        </table>
      </div>
    `
  }

  override render() {
    const snapshot = this.snapshot
    return html`
      <section class="volumes" part="volumes" aria-busy=${this.loading || this.busy}>
        <header class="toolbar" part="toolbar">
          <div class="status">
            <span class="dot" data-ready=${snapshot !== undefined}></span>
            <span>${snapshot ? 'inventory loaded' : 'no snapshot'}</span>
            ${snapshot
              ? html`<span class="muted">
                  ${deviceCountLabel(snapshot)} · ${mountCountLabel(snapshot)}
                </span>`
              : nothing}
          </div>
          <span class="spacer"></span>
          <input
            type="search"
            class="filter"
            placeholder="Filter devices and mounts"
            aria-label="Filter devices and mounts"
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
              Volume control is disabled by the daemon. The inventory stays
              read-only.
            </p>`}

        ${this.renderDevices()} ${this.renderMounts()}
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

    .volumes {
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
    .mount-form {
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
    input:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .danger {
      color: #fca5a5;
    }

    .filter {
      min-width: 12rem;
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

    .mount-form {
      padding: 9px 10px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: #0c131c;
    }

    .mount-form input[type='text'] {
      flex: 1;
      min-width: 12rem;
    }

    .mount-form input.type {
      flex: 0 1 9rem;
    }

    .check {
      display: inline-flex;
      align-items: center;
      gap: 5px;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
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

    .table-wrap {
      overflow: auto;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
    }

    table {
      width: 100%;
      min-width: 52rem;
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

    .mono {
      font-family: var(--mono, monospace);
    }

    .device {
      padding-left: calc(9px + var(--depth, 0) * 1.1rem);
    }

    .device .name {
      color: var(--text, #dbe6f2);
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

    .bar {
      display: inline-block;
      width: 3rem;
      height: 4px;
      margin-left: 6px;
      border-radius: 2px;
      vertical-align: middle;
      background: linear-gradient(
        to right,
        #38bdf8 calc(var(--percent, 0) * 1%),
        #1e293b calc(var(--percent, 0) * 1%)
      );
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
    'anth-volume-manager': AnthVolumeManager
  }
}
