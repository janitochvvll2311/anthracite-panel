/**
 * `anth-storage`: the read-only storage inventory.
 *
 * The component is mounted twice by the Storage section: once with
 * `view="volumes"` and once with `view="partitions"`. Both ask `/api/volumes`
 * for the same snapshot; there is no SSE because storage changes are rare and
 * the panel only reads in this slice.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import { describeError } from './errors'
import { formatBytes, formatPercent } from './format'
import {
  deviceRows,
  filterDevices,
  filterMounts,
  isSwap,
  mountUsagePercent,
  mountedCount,
  partitionRows,
} from './lib/storage/display'
import type { DeviceRow } from './lib/storage/display'
import type { StorageClient, VolumeDevice, VolumeMount, VolumeSnapshot } from './storage-client'

type StorageView = 'volumes' | 'partitions'
type LoadStatus = 'idle' | 'loading' | 'ready' | 'error'

@customElement('anth-storage')
@localized()
export class AnthStorage extends LitElement {
  @property({ attribute: false })
  client?: StorageClient

  @property({ type: String })
  view: StorageView = 'volumes'

  @state()
  private snapshot?: VolumeSnapshot

  @state()
  private status: LoadStatus = 'idle'

  @state()
  private loading = false

  @state()
  private error = ''

  @state()
  private query = ''

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
    } catch (failure) {
      this.error = describeError(failure)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private statusLabel(): string {
    if (this.loading) return msg('loading')
    if (this.status === 'error') return msg('error')
    if (!this.snapshot) return msg('idle')
    return msg('ready')
  }

  private countLabel(): string {
    if (!this.snapshot) return ''
    if (this.view === 'partitions') {
      const count = partitionRows(this.snapshot.devices).length
      return count === 1 ? msg('1 partition') : msg(str`${count} partitions`)
    }
    const devices = this.snapshot.devices.length
    const mounted = mountedCount(this.snapshot.devices)
    if (devices === 1 && mounted === 1) return msg('1 device · 1 mounted')
    if (devices === 1) return msg(str`1 device · ${mounted} mounted`)
    return msg(str`${devices} devices · ${mounted} mounted`)
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
      <span class="policy" data-on=${enabled ? 'yes' : 'no'}>
        ${label}
      </span>
    `
  }

  private renderContext() {
    const snapshot = this.snapshot
    if (!snapshot) return nothing
    return html`
      <div class="context" part="context">
        <span class="context-label">${msg('Mount root')}</span>
        <span class="mono">${snapshot.mount_root}</span>
        <span class="policies">
          ${this.policy(msg('control'), snapshot.control_enabled)}
          ${this.policy(msg('partition'), snapshot.partition_enabled)}
          ${this.policy(msg('format'), snapshot.format_enabled)}
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

  private mountChips(device: VolumeDevice) {
    if (device.mountpoints.length === 0) return nothing
    return html`
      <div class="chips">
        ${device.mountpoints.map(
          (mountpoint) => html`<span class="chip">${mountpoint}</span>`,
        )}
      </div>
    `
  }

  private renderDevice(device: DeviceRow, snapshot: VolumeSnapshot) {
    return html`
      <article class="device" data-kind=${device.kind} data-mounted=${device.mountpoints.length > 0 ? 'yes' : 'no'}>
        <div class="identity" style=${`padding-left: ${device.depth * 0.9}rem`}>
          <div class="title">
            <span class="dot" aria-hidden="true"></span>
            <h3>${device.name}</h3>
            ${this.badge(device.kind)}
            ${device.read_only ? this.badge(msg('read-only'), 'warn') : nothing}
            ${device.removable ? this.badge(msg('removable'), 'warn') : nothing}
            ${isSwap(device, snapshot.swap_devices) ? this.badge(msg('swap'), 'warn') : nothing}
          </div>
          <p class="meta">
            <span>${formatBytes(device.size_bytes)}</span>
            <span>${device.fstype ?? '—'}</span>
            ${device.label ? html`<span class="label">${device.label}</span>` : nothing}
            ${device.model ? html`<span class="model">${device.model}</span>` : nothing}
            ${device.transport ? html`<span>${device.transport}</span>` : nothing}
          </p>
        </div>
        <div class="mounts-cell">${this.mountChips(device)}</div>
        <code class="path">${device.path}</code>
      </article>
    `
  }

  private renderMount(mount: VolumeMount) {
    const usage = mount.usage
    const percent = usage ? mountUsagePercent(usage) : 0
    return html`
      <article class="mount" part="mount">
        <div class="mount-name">
          <h4>${mount.mountpoint}</h4>
          <p class="meta">
            <span>${mount.source}</span>
            <span>${mount.fstype}</span>
            ${mount.read_only ? this.badge(msg('read-only'), 'warn') : nothing}
            ${mount.virtual_fs ? this.badge(msg('virtual')) : nothing}
          </p>
        </div>
        <div class="usage">
          ${usage
            ? html`
                <div class="bar" role="img" aria-label=${`${formatPercent(percent)} ${msg('used')}`}>
                  <span style=${`width: ${percent}%`}></span>
                </div>
                <p class="usage-text">
                  ${formatBytes(usage.used_bytes)} / ${formatBytes(usage.total_bytes)}
                  · ${formatPercent(percent)}
                </p>
              `
            : html`<p class="muted">—</p>`}
        </div>
      </article>
    `
  }

  private renderVolumes(snapshot: VolumeSnapshot) {
    const rows = filterDevices(deviceRows(snapshot.devices), this.query)
    const mounts = filterMounts(snapshot.mounts, this.query)
    return html`
      ${this.renderFilter(msg('Filter devices or mounts'))}
      ${rows.length === 0
        ? html`<p class="empty">
            ${snapshot.devices.length === 0
              ? msg('No block devices found.')
              : msg('No devices match the filter.')}
          </p>`
        : html`<div class="list" part="devices">${rows.map((device) => this.renderDevice(device, snapshot))}</div>`}
      <h3 class="section-title">${msg('Mounts')}</h3>
      ${mounts.length === 0
        ? html`<p class="empty">
            ${snapshot.mounts.length === 0
              ? msg('No mounts found.')
              : msg('No mounts match the filter.')}
          </p>`
        : html`<div class="mount-list" part="mounts">${mounts.map((mount) => this.renderMount(mount))}</div>`}
      ${snapshot.swap_devices.length > 0
        ? html`
            <h3 class="section-title">${msg('Swap')}</h3>
            <div class="chips">
              ${snapshot.swap_devices.map((device) => html`<span class="chip">${device}</span>`)}
            </div>
          `
        : nothing}
    `
  }

  private renderPartitions(snapshot: VolumeSnapshot) {
    const all = deviceRows(snapshot.devices)
    const filtered = filterDevices(all, this.query).filter((device) => device.kind === 'part')
    const parent = new Map(snapshot.devices.map((device) => [device.name, device.path]))
    return html`
      ${this.renderFilter(msg('Filter partitions'))}
      ${filtered.length === 0
        ? html`<p class="empty">
            ${partitionRows(snapshot.devices).length === 0
              ? msg('No partitions found.')
              : msg('No partitions match the filter.')}
          </p>`
        : html`
            <div class="list" part="partitions">
              ${filtered.map(
                (device) => html`
                  <article class="device partition" data-kind="part">
                    <div class="identity">
                      <div class="title">
                        <span class="dot" aria-hidden="true"></span>
                        <h3>${device.name}</h3>
                        ${this.badge(msg('partition'))}
                        ${device.read_only ? this.badge(msg('read-only'), 'warn') : nothing}
                      </div>
                      <p class="meta">
                        <span>${device.parent ? parent.get(device.parent) ?? device.parent : '—'}</span>
                        <span>${formatBytes(device.size_bytes)}</span>
                        <span>${device.fstype ?? '—'}</span>
                      </p>
                    </div>
                    <div class="mounts-cell">${this.mountChips(device)}</div>
                    <code class="path">${device.path}</code>
                  </article>
                `,
              )}
            </div>
          `}
    `
  }

  private renderBody() {
    if (!this.snapshot) {
      if (this.error) return html`<p class="error" role="alert">${this.error}</p>`
      if (this.client) return html`<p class="empty">${msg('Reading storage…')}</p>`
      return html`<p class="empty">${msg('No storage information yet.')}</p>`
    }
    const snapshot = this.snapshot
    return html`
      ${snapshot.warning
        ? html`<p class="warning" role="status">${snapshot.warning}</p>`
        : nothing}
      ${this.error ? html`<p class="error" role="alert">${this.error}</p>` : nothing}
      ${this.renderContext()}
      ${this.view === 'partitions' ? this.renderPartitions(snapshot) : this.renderVolumes(snapshot)}
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
    .mono { color: var(--text, #dbe6f2); }
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
    .toolbar button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }

    .list { display: flex; flex-direction: column; gap: 0.5rem; }
    .device {
      display: grid;
      grid-template-columns: minmax(14rem, 1.25fr) minmax(10rem, 1fr) auto;
      align-items: center;
      gap: 0.45rem 0.9rem;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-left: 3px solid var(--muted, #8b9bad);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .device[data-mounted='yes'] { border-left-color: var(--ok, #34d399); }
    .device[data-kind='disk'] { border-left-color: var(--accent, #7dd3fc); }
    .device[data-kind='disk'][data-mounted='yes'] { border-left-color: var(--ok, #34d399); }
    .device .title { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem; }
    .device h3 { margin: 0; font: 600 15px/1.3 var(--mono, monospace); }
    .dot { width: 7px; height: 7px; border-radius: 999px; background: var(--muted, #8b9bad); }
    .device[data-mounted='yes'] .dot { background: var(--ok, #34d399); }
    .device[data-kind='disk'] .dot { background: var(--accent, #7dd3fc); }
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
    }
    .meta .model,
    .meta .label { overflow-wrap: anywhere; }
    .mounts-cell { min-width: 0; }
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
    .path { color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); text-align: right; }

    .section-title {
      margin: 0.35rem 0 0;
      color: var(--muted, #8b9bad);
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }
    .mount-list { display: flex; flex-direction: column; gap: 0.4rem; }
    .mount {
      display: grid;
      grid-template-columns: minmax(14rem, 1.2fr) minmax(12rem, 1fr);
      gap: 0.4rem 1rem;
      align-items: center;
      padding: 0.5rem 0.65rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel-2, #0b121b);
    }
    .mount h4 { margin: 0; font: 600 13px/1.4 var(--mono, monospace); overflow-wrap: anywhere; }
    .usage { display: flex; flex-direction: column; gap: 0.2rem; }
    .bar {
      height: 6px;
      overflow: hidden;
      border-radius: 999px;
      background: var(--panel-border, #223041);
    }
    .bar span { display: block; height: 100%; background: var(--accent, #7dd3fc); }
    .usage-text { margin: 0; color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); }

    .muted { color: var(--muted, #8b9bad); }
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
      .device,
      .mount { grid-template-columns: 1fr; align-items: start; }
      .device .identity { padding-left: 0 !important; }
      .path { text-align: left; }
      .policies { margin-left: 0; }
    }
  `
}
