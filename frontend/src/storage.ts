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
  allowedDisks,
  deviceRows,
  filterDevices,
  filterMounts,
  isSwap,
  mountUsagePercent,
  mountedCount,
  partitionAllowed,
  partitionNumber,
  partitionRows,
} from './lib/storage/display'
import type { DeviceRow } from './lib/storage/display'
import type {
  PartitionKind,
  StorageClient,
  VolumeDevice,
  VolumeFormatRequest,
  VolumeMount,
  VolumeMountRequest,
  VolumePartitionCreate,
  VolumePartitionDelete,
  VolumePartitionResize,
  VolumeSnapshot,
} from './storage-client'

type StorageView = 'volumes' | 'partitions'

/** Filesystems the panel may create. */
const FORMAT_TYPES = ['ext4', 'xfs', 'vfat', 'exfat', 'btrfs', 'swap'] as const
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

  @state()
  private busy = false

  @state()
  private actionError = ''

  @state()
  private notice = ''

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

  @state()
  private confirmingUnmount?: string

  @state()
  private createDisk = ''

  @state()
  private createKind: PartitionKind = 'linux'

  @state()
  private createSize = ''

  @state()
  private resizing?: string

  @state()
  private formatting?: string

  @state()
  private formatType = 'ext4'

  @state()
  private confirmingPartitionDelete?: string

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
      this.actionError = ''
      this.notice = ''
      this.syncMountDevice()
      this.syncCreateDisk()
    } catch (failure) {
      this.error = describeError(failure)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private syncCreateDisk(): void {
    const disks = this.editableDisks()
    if (!disks.some((disk) => disk.path === this.createDisk)) {
      this.createDisk = disks[0]?.path ?? ''
    }
  }

  private editableDisks(): VolumeDevice[] {
    if (!this.snapshot) return []
    return allowedDisks(this.snapshot.devices, this.snapshot.disk_allow)
  }

  private canPartition(device: VolumeDevice): boolean {
    return (
      this.snapshot?.partition_enabled === true &&
      partitionAllowed(device, this.snapshot.disk_allow) &&
      partitionNumber(device.name) !== undefined
    )
  }

  private canFormat(device: VolumeDevice): boolean {
    return (
      this.snapshot?.format_enabled === true &&
      partitionAllowed(device, this.snapshot.disk_allow)
    )
  }

  /** Keeps the device picker on an unmounted device after a snapshot. */
  private syncMountDevice(): void {
    const candidates = this.unmountedDevices()
    if (!candidates.some((device) => device.path === this.mountDevice)) {
      this.mountDevice = candidates[0]?.path ?? ''
    }
  }

  private unmountedDevices(): VolumeDevice[] {
    if (!this.snapshot) return []
    return this.snapshot.devices.filter(
      (device) => device.mountpoints.length === 0 && device.kind !== 'loop',
    )
  }

  private async run(
    task: (client: StorageClient) => Promise<unknown>,
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
      this.snapshot = await client.get()
      this.status = 'ready'
      this.error = ''
      this.notice = msg('Change applied.')
      this.syncMountDevice()
      this.syncCreateDisk()
    } catch (failure) {
      this.actionError = describeError(failure)
    } finally {
      this.busy = false
    }
  }

  private submitMount(event: Event): void {
    event.preventDefault()
    if (this.busy) return
    if (this.mountDevice === '' || this.mountTarget.trim() === '') {
      this.actionError = msg('Select a device and a target directory.')
      return
    }
    const request: VolumeMountRequest = {
      device: this.mountDevice,
      target: this.mountTarget.trim(),
      read_only: this.mountReadOnly,
      allow_removable: this.mountAllowRemovable,
    }
    const fsType = this.mountType.trim()
    if (fsType !== '') request.fs_type = fsType
    void this.run(
      (client) => client.mount(request),
      () => {
        this.mountTarget = ''
        this.mountType = ''
        this.mountReadOnly = false
        this.mountAllowRemovable = false
      },
    )
  }

  private unmount(target: string): void {
    void this.run(
      (client) => client.unmount(target),
      () => {
        this.confirmingUnmount = undefined
      },
    )
  }

  private sizeBytes(value: string): number | undefined {
    const mib = Number(value)
    if (!Number.isSafeInteger(mib) || mib <= 0) return undefined
    const bytes = mib * 1024 * 1024
    return Number.isSafeInteger(bytes) ? bytes : undefined
  }

  private parentDisk(device: VolumeDevice): VolumeDevice | undefined {
    return this.snapshot?.devices.find(
      (candidate) => candidate.kind === 'disk' && candidate.name === device.parent,
    )
  }

  private submitCreatePartition(event: Event): void {
    event.preventDefault()
    if (this.busy) return
    const size = this.sizeBytes(this.createSize)
    if (this.createDisk === '' || size === undefined) {
      this.actionError = msg('Choose an allowed disk and a positive size in MiB.')
      return
    }
    const request: VolumePartitionCreate = {
      disk: this.createDisk,
      kind: this.createKind,
      size_bytes: size,
    }
    void this.run(
      (client) => client.createPartition(request),
      () => {
        this.createSize = ''
      },
    )
  }

  private submitResize(event: Event, device: VolumeDevice): void {
    event.preventDefault()
    if (this.busy) return
    const number = partitionNumber(device.name)
    const disk = this.parentDisk(device)
    const form = event.currentTarget as HTMLFormElement
    const size = this.sizeBytes(
      String(new FormData(form).get('size_mib') ?? ''),
    )
    if (!number || !disk || size === undefined) {
      this.actionError = msg('Choose a positive new size in MiB.')
      return
    }
    const request: VolumePartitionResize = {
      disk: disk.path,
      number,
      size_bytes: size,
    }
    void this.run(
      (client) => client.resizePartition(request),
      () => {
        this.resizing = undefined
      },
    )
  }

  private submitFormat(event: Event, device: VolumeDevice): void {
    event.preventDefault()
    if (this.busy) return
    const form = event.currentTarget as HTMLFormElement
    const request: VolumeFormatRequest = {
      partition: device.path,
      fs_type: this.formatType,
    }
    const label = String(new FormData(form).get('label') ?? '').trim()
    if (label !== '') request.label = label
    void this.run(
      (client) => client.formatPartition(request),
      () => {
        this.formatting = undefined
      },
    )
  }

  private submitPartitionDelete(device: VolumeDevice): void {
    const number = partitionNumber(device.name)
    const disk = this.parentDisk(device)
    if (!number || !disk) return
    const request: VolumePartitionDelete = { disk: disk.path, number }
    void this.run(
      (client) => client.deletePartition(request),
      () => {
        this.confirmingPartitionDelete = undefined
      },
    )
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
    const control = this.snapshot?.control_enabled === true
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
        ${control && !mount.virtual_fs && mount.mountpoint !== '/'
          ? html`
              <button
                type="button"
                class="danger unmount"
                ?disabled=${this.busy}
                title=${msg(str`Unmount ${mount.mountpoint}`)}
                @click=${() => {
                  this.confirmingUnmount =
                    this.confirmingUnmount === mount.mountpoint ? undefined : mount.mountpoint
                }}
              >
                ${msg('Unmount')}
              </button>
            `
          : nothing}
        ${this.confirmingUnmount === mount.mountpoint
          ? html`
              <div class="confirm" role="alert">
                <span>${msg(str`Unmount ${mount.mountpoint}?`)}</span>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => this.unmount(mount.mountpoint)}
                >
                  ${msg('Unmount')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.confirmingUnmount = undefined
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

  private renderMountForm(snapshot: VolumeSnapshot) {
    const candidates = this.unmountedDevices()
    return html`
      <form class="mount-form" part="mount-form" @submit=${(event: Event) => this.submitMount(event)}>
        <label>
          <span>${msg('Device')}</span>
          <select
            name="device"
            .value=${this.mountDevice}
            ?disabled=${this.busy || candidates.length === 0}
            @change=${(event: Event) => {
              this.mountDevice = (event.currentTarget as HTMLSelectElement).value
            }}
          >
            ${candidates.length === 0
              ? html`<option value="">${msg('No unmounted devices')}</option>`
              : candidates.map(
                  (device) => html`
                    <option value=${device.path}>
                      ${device.name} · ${formatBytes(device.size_bytes)}${device.fstype
                        ? ` · ${device.fstype}`
                        : ''}
                    </option>
                  `,
                )}
          </select>
        </label>
        <label>
          <span>${msg('Target directory')}</span>
          <input
            name="target"
            .value=${this.mountTarget}
            placeholder=${`${snapshot.mount_root}/data`}
            ?disabled=${this.busy}
            @input=${(event: Event) => {
              this.mountTarget = (event.currentTarget as HTMLInputElement).value
            }}
          />
        </label>
        <label>
          <span>${msg('Type (optional)')}</span>
          <input
            name="fs_type"
            .value=${this.mountType}
            placeholder=${msg('auto')}
            ?disabled=${this.busy}
            @input=${(event: Event) => {
              this.mountType = (event.currentTarget as HTMLInputElement).value
            }}
          />
        </label>
        <label class="check">
          <input
            name="read_only"
            type="checkbox"
            .checked=${this.mountReadOnly}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.mountReadOnly = (event.currentTarget as HTMLInputElement).checked
            }}
          />
          <span>${msg('Read-only')}</span>
        </label>
        <label class="check">
          <input
            name="allow_removable"
            type="checkbox"
            .checked=${this.mountAllowRemovable}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.mountAllowRemovable = (event.currentTarget as HTMLInputElement).checked
            }}
          />
          <span>${msg('Allow removable')}</span>
        </label>
        <button
          type="submit"
          ?disabled=${this.busy || candidates.length === 0 || this.mountTarget.trim() === ''}
        >
          ${msg('Mount')}
        </button>
      </form>
      <p class="hint">
        ${msg('The target must be an existing directory under the configured mount root.')}
      </p>
    `
  }

  private renderVolumes(snapshot: VolumeSnapshot) {
    const rows = filterDevices(deviceRows(snapshot.devices), this.query)
    const mounts = filterMounts(snapshot.mounts, this.query)
    return html`
      ${snapshot.control_enabled ? this.renderMountForm(snapshot) : nothing}
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

  private renderPartitionCreate(snapshot: VolumeSnapshot) {
    if (!snapshot.partition_enabled) return nothing
    const disks = this.editableDisks()
    if (disks.length === 0) {
      return html`
        <p class="empty">
          ${msg('No disk is listed in disk_allow, so partition edits are unavailable.')}
        </p>
      `
    }
    return html`
      <form
        class="partition-create"
        part="partition-create"
        @submit=${(event: Event) => this.submitCreatePartition(event)}
      >
        <label>
          <span>${msg('Disk device')}</span>
          <select
            name="disk"
            .value=${this.createDisk}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.createDisk = (event.currentTarget as HTMLSelectElement).value
            }}
          >
            ${disks.map(
              (disk) => html`<option value=${disk.path}>
                ${disk.name} · ${formatBytes(disk.size_bytes)}
              </option>`,
            )}
          </select>
        </label>
        <label>
          <span>${msg('Kind')}</span>
          <select
            name="kind"
            .value=${this.createKind}
            ?disabled=${this.busy}
            @change=${(event: Event) => {
              this.createKind = (event.currentTarget as HTMLSelectElement)
                .value as PartitionKind
            }}
          >
            <option value="linux">linux</option>
            <option value="swap">swap</option>
            <option value="efi">efi</option>
          </select>
        </label>
        <label>
          <span>${msg('Size (MiB)')}</span>
          <input
            name="size_mib"
            type="number"
            min="1"
            .value=${this.createSize}
            ?disabled=${this.busy}
            @input=${(event: Event) => {
              this.createSize = (event.currentTarget as HTMLInputElement).value
            }}
          />
        </label>
        <button
          type="submit"
          ?disabled=${this.busy || this.createSize.trim() === ''}
        >
          ${msg('Create partition')}
        </button>
      </form>
      <p class="hint">
        ${msg('Sizes are whole MiB. The disk must be in disk_allow and unmounted.')}
      </p>
    `
  }

  private renderPartitionActions(device: VolumeDevice) {
    const resize = this.canPartition(device)
    const format = this.canFormat(device)
    if (!resize && !format) return nothing
    return html`
      <div class="manage">
        <div class="actions">
          ${resize
            ? html`
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.resizing =
                      this.resizing === device.path ? undefined : device.path
                    this.formatting = undefined
                    this.confirmingPartitionDelete = undefined
                  }}
                >
                  ${msg('Resize')}
                </button>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.confirmingPartitionDelete =
                      this.confirmingPartitionDelete === device.path
                        ? undefined
                        : device.path
                    this.resizing = undefined
                    this.formatting = undefined
                  }}
                >
                  ${msg('Delete')}
                </button>
              `
            : nothing}
          ${format
            ? html`
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.formatting =
                      this.formatting === device.path ? undefined : device.path
                    this.resizing = undefined
                    this.confirmingPartitionDelete = undefined
                  }}
                >
                  ${msg('Format')}
                </button>
              `
            : nothing}
        </div>
        ${this.resizing === device.path
          ? html`
              <form
                class="partition-inline"
                @submit=${(event: Event) => this.submitResize(event, device)}
              >
                <label>
                  <span>${msg('New size (MiB)')}</span>
                  <input
                    name="size_mib"
                    type="number"
                    min="1"
                    ?disabled=${this.busy}
                  />
                </label>
                <button type="submit" ?disabled=${this.busy}>${msg('Apply')}</button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.resizing = undefined
                  }}
                >
                  ${msg('Cancel')}
                </button>
              </form>
            `
          : nothing}
        ${this.formatting === device.path
          ? html`
              <form
                class="partition-inline"
                @submit=${(event: Event) => this.submitFormat(event, device)}
              >
                <label>
                  <span>${msg('Filesystem')}</span>
                  <select
                    .value=${this.formatType}
                    ?disabled=${this.busy}
                    @change=${(event: Event) => {
                      this.formatType = (event.currentTarget as HTMLSelectElement).value
                    }}
                  >
                    ${FORMAT_TYPES.map((type) => html`<option value=${type}>${type}</option>`)}
                  </select>
                </label>
                <label>
                  <span>${msg('Label (optional)')}</span>
                  <input
                    name="label"
                    ?disabled=${this.busy}
                  />
                </label>
                <button type="submit" ?disabled=${this.busy}>${msg('Format')}</button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.formatting = undefined
                  }}
                >
                  ${msg('Cancel')}
                </button>
              </form>
            `
          : nothing}
        ${this.confirmingPartitionDelete === device.path
          ? html`
              <div class="confirm" role="alert">
                <span>${msg(str`Delete partition ${device.name}? This cannot be undone.`)}</span>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => this.submitPartitionDelete(device)}
                >
                  ${msg('Delete')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.confirmingPartitionDelete = undefined
                  }}
                >
                  ${msg('Cancel')}
                </button>
              </div>
            `
          : nothing}
      </div>
    `
  }

  private renderPartitions(snapshot: VolumeSnapshot) {
    const all = deviceRows(snapshot.devices)
    const filtered = filterDevices(all, this.query).filter((device) => device.kind === 'part')
    const parent = new Map(snapshot.devices.map((device) => [device.name, device.path]))
    return html`
      ${this.renderPartitionCreate(snapshot)}
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
                    ${this.renderPartitionActions(device)}
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
      ${this.actionError
        ? html`<p class="error" role="alert">${this.actionError}</p>`
        : nothing}
      ${this.notice ? html`<p class="notice" role="status">${this.notice}</p>` : nothing}
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

    .mount-form {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
      align-items: end;
      gap: 0.5rem 0.75rem;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .mount-form label {
      display: grid;
      gap: 0.2rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }
    .mount-form input,
    .mount-form select {
      min-height: 32px;
      padding: 0.3rem 0.5rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
    }
    .mount-form .check {
      display: flex;
      grid-auto-flow: column;
      align-items: center;
      align-self: end;
      gap: 0.35rem;
      min-height: 32px;
    }
    .mount-form .check input { min-height: 0; width: auto; }
    .mount-form button {
      min-height: 32px;
      padding: 0.3rem 0.65rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .mount-form button:hover { border-color: var(--accent, #7dd3fc); }
    .mount-form button:disabled { opacity: 0.55; cursor: progress; }
    .hint { margin: -0.25rem 0 0; color: var(--muted, #8b9bad); font-size: 11px; }

    .partition-create {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
      align-items: end;
      gap: 0.5rem 0.75rem;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel, #101721);
    }
    .partition-create label,
    .partition-inline label {
      display: grid;
      gap: 0.2rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }
    .partition-create input,
    .partition-create select,
    .partition-inline input,
    .partition-inline select {
      min-height: 32px;
      padding: 0.3rem 0.5rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: 13px/1.4 var(--sans, system-ui, sans-serif);
    }
    .partition-create button,
    .partition-inline button,
    .actions button {
      min-height: 32px;
      padding: 0.3rem 0.6rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel-2, #0b121b);
      font: inherit;
      cursor: pointer;
    }
    .partition-create button:hover,
    .partition-inline button:hover,
    .actions button:hover { border-color: var(--accent, #7dd3fc); }
    .partition-create button:disabled,
    .partition-inline button:disabled,
    .actions button:disabled { opacity: 0.55; cursor: progress; }
    .actions button.danger { color: var(--err, #f87171); }
    .manage {
      grid-column: 1 / -1;
      display: flex;
      flex-direction: column;
      gap: 0.45rem;
      padding-top: 0.5rem;
      border-top: 1px solid var(--panel-border, #223041);
    }
    .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 0.4rem; }
    .partition-inline { display: flex; flex-wrap: wrap; align-items: end; gap: 0.4rem; }

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
      grid-template-columns: minmax(14rem, 1.2fr) minmax(12rem, 1fr) auto;
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

    .unmount {
      min-height: 30px;
      padding: 0.2rem 0.5rem;
      color: var(--err, #f87171);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel, #101721);
      font: inherit;
      cursor: pointer;
    }
    .unmount:hover { border-color: var(--err, #f87171); }
    .unmount:disabled { opacity: 0.55; cursor: progress; }
    .mount .confirm { grid-column: 1 / -1; }
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
    .confirm button.danger { color: var(--err, #f87171); }
    .confirm button:disabled { opacity: 0.55; cursor: progress; }

    .muted { color: var(--muted, #8b9bad); }
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
      .toolbar button { min-height: 44px; }
      .filters input { min-height: 44px; }
      .mount-form,
      .partition-create { grid-template-columns: 1fr 1fr; }
      .mount-form button,
      .partition-create button { min-height: 44px; }
      .mount-form input,
      .mount-form select,
      .partition-create input,
      .partition-create select,
      .partition-inline input,
      .partition-inline select { min-height: 44px; }
      .actions { justify-content: flex-start; }
      .actions button,
      .partition-inline button { min-height: 44px; }
      .device,
      .mount { grid-template-columns: 1fr; align-items: start; }
      .device .identity { padding-left: 0 !important; }
      .path { text-align: left; }
      .policies { margin-left: 0; }
      .unmount,
      .confirm button { min-height: 44px; }
    }
  `
}
