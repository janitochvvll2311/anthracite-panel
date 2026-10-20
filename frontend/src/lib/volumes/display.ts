import type {
  VolumeDevice,
  VolumeKind,
  VolumeMountpoint,
  VolumeSnapshot,
} from './volume-client'
import { formatBytes } from '../format'

export { formatBytes } from '../format'

export type VolumeSortKey = 'device' | 'size' | 'type' | 'fstype' | 'label' | 'mount'
export type VolumeSortDirection = 'asc' | 'desc'
export type VolumeMountSortKey =
  | 'mountpoint'
  | 'source'
  | 'fstype'
  | 'used'
  | 'available'
export type VolumeMountSortDirection = 'asc' | 'desc'

/** A device plus the indentation derived from its parent links. */
export interface DeviceRow extends VolumeDevice {
  depth: number
  mountLabel: string
}

const KIND_LABELS: Record<VolumeKind, string> = {
  disk: 'disk',
  part: 'part',
  crypt: 'crypt',
  lvm: 'lvm',
  raid: 'raid',
  loop: 'loop',
  rom: 'rom',
  other: 'other',
}

export function kindLabel(kind: VolumeKind): string {
  return KIND_LABELS[kind] ?? 'other'
}

/**
 * The daemon always sends `mountpoints`, but an older daemon omitted the key
 * for an unmounted device. Defaulting here keeps one payload from breaking the
 * whole table.
 */
export function mountpointsOf(device: VolumeDevice): string[] {
  return device.mountpoints ?? []
}

export function mountLabel(device: VolumeDevice): string {
  const points = mountpointsOf(device)
  return points.length === 0 ? '—' : points.join(', ')
}

/**
 * Walks the parent links to compute nesting. The protocol carries the tree
 * flat; the component draws it. The depth is capped by the list length so a
 * malformed `parent` cannot loop forever.
 */
export function deviceDepth(
  devices: VolumeDevice[],
  device: VolumeDevice,
): number {
  const byName = new Map(devices.map((entry) => [entry.name, entry]))
  let depth = 0
  let current = device

  while (current.parent !== undefined) {
    depth += 1
    if (depth >= devices.length) break
    const parent = byName.get(current.parent)
    if (parent === undefined) break
    current = parent
  }

  return depth
}

export function toDeviceRows(devices: VolumeDevice[]): DeviceRow[] {
  return devices.map((device) => ({
    ...device,
    depth: deviceDepth(devices, device),
    mountLabel: mountLabel(device),
  }))
}

/**
 * Preserves the daemon's tree order while filtering. A matching child keeps its
 * ancestors visible, so the indentation still reads as a tree.
 */
export function filterDeviceRows(rows: DeviceRow[], filter: string): DeviceRow[] {
  const needle = filter.trim().toLowerCase()
  if (needle === '') return rows

  const byName = new Map(rows.map((row) => [row.name, row]))
  const keep = new Set<string>()

  const matches = (row: DeviceRow): boolean =>
    [
      row.name,
      row.path,
      row.kind,
      row.fstype ?? '',
      row.label ?? '',
      row.uuid ?? '',
      row.model ?? '',
      row.mountLabel,
    ].some((value) => value.toLowerCase().includes(needle))

  for (const row of rows) {
    if (!matches(row)) continue
    keep.add(row.name)
    let parent = row.parent
    while (parent !== undefined && !keep.has(parent)) {
      keep.add(parent)
      parent = byName.get(parent)?.parent
    }
  }

  return rows.filter((row) => keep.has(row.name))
}

export function filterMounts(
  mounts: VolumeMountpoint[],
  filter: string,
): VolumeMountpoint[] {
  const needle = filter.trim().toLowerCase()
  if (needle === '') return mounts

  return mounts.filter((mount) =>
    [mount.mountpoint, mount.source, mount.fstype].some((value) =>
      value.toLowerCase().includes(needle),
    ),
  )
}

export function usedPercent(mount: VolumeMountpoint): number | null {
  const usage = mount.usage
  if (usage === undefined || usage.total_bytes === 0) return null
  return (usage.used_bytes / usage.total_bytes) * 100
}

export function usageLabel(mount: VolumeMountpoint): string {
  const usage = mount.usage
  if (usage === undefined) return '—'
  const percent = usedPercent(mount)
  const used = formatBytes(usage.used_bytes)
  return percent === null ? used : `${used} (${percent.toFixed(0)}%)`
}

export function availableLabel(mount: VolumeMountpoint): string {
  return mount.usage === undefined ? '—' : formatBytes(mount.usage.available_bytes)
}

export function mountMode(mount: VolumeMountpoint): string {
  if (mount.virtual_fs) return mount.read_only ? 'ro, virtual' : 'virtual'
  return mount.read_only ? 'ro' : 'rw'
}

/** Devices that can be offered in the mount form: unmounted, with a filesystem. */
export function mountableDevices(devices: VolumeDevice[]): VolumeDevice[] {
  return devices.filter(
    (device) =>
      mountpointsOf(device).length === 0 &&
      device.fstype !== undefined &&
      device.kind !== 'disk',
  )
}

function compare(
  a: string | number | null,
  b: string | number | null,
  direction: 'asc' | 'desc',
  leftTie: string,
  rightTie: string,
): number {
  if (a === null && b === null) return leftTie.localeCompare(rightTie)
  if (a === null) return 1
  if (b === null) return -1
  if (a === b) return leftTie.localeCompare(rightTie)
  const order = direction === 'asc' ? 1 : -1
  if (typeof a === 'string' && typeof b === 'string') {
    return a.localeCompare(b) * order
  }
  return (Number(a) - Number(b)) * order
}

export function compareDeviceRows(
  left: DeviceRow,
  right: DeviceRow,
  key: VolumeSortKey,
  direction: VolumeSortDirection,
): number {
  const value = (row: DeviceRow): string | number | null => {
    switch (key) {
      case 'device':
        return row.name
      case 'size':
        return row.size_bytes
      case 'type':
        return kindLabel(row.kind)
      case 'fstype':
        return row.fstype ?? null
      case 'label':
        return row.label ?? null
      case 'mount':
        return mountpointsOf(row).length === 0 ? null : row.mountLabel
    }
  }
  return compare(value(left), value(right), direction, left.name, right.name)
}

export function compareMounts(
  left: VolumeMountpoint,
  right: VolumeMountpoint,
  key: VolumeMountSortKey,
  direction: VolumeMountSortDirection,
): number {
  const value = (mount: VolumeMountpoint): string | number | null => {
    switch (key) {
      case 'mountpoint':
        return mount.mountpoint
      case 'source':
        return mount.source
      case 'fstype':
        return mount.fstype
      case 'used':
        return mount.usage?.used_bytes ?? null
      case 'available':
        return mount.usage?.available_bytes ?? null
    }
  }
  return compare(
    value(left),
    value(right),
    direction,
    left.mountpoint,
    right.mountpoint,
  )
}

export function deviceCountLabel(snapshot: VolumeSnapshot): string {
  const noun = snapshot.total === 1 ? 'device' : 'devices'
  if (snapshot.truncated) return `${snapshot.devices.length} of ${snapshot.total} ${noun}`
  return `${snapshot.devices.length} ${noun}`
}

export function mountCountLabel(snapshot: VolumeSnapshot): string {
  const noun = snapshot.mounts.length === 1 ? 'mount' : 'mounts'
  return `${snapshot.mounts.length} ${noun}`
}
