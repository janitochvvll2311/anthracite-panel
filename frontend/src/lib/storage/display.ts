/**
 * Derived storage values: device depth, partition rows, mounted counts and
 * filtering. The daemon sends flat device lists with parent links.
 */

import type { VolumeDevice, VolumeMount } from '../../storage-client'

export interface DeviceRow extends VolumeDevice {
  /** Nesting depth from the parent chain; disks are zero. */
  depth: number
}

/** Adds a depth to every device while preserving the lsblk order. */
export function deviceRows(devices: VolumeDevice[]): DeviceRow[] {
  const byName = new Map(devices.map((device) => [device.name, device]))
  const depths = new Map<string, number>()

  const depthOf = (name: string, seen: Set<string>): number => {
    const cached = depths.get(name)
    if (cached !== undefined) return cached
    const device = byName.get(name)
    if (!device || !device.parent || seen.has(name)) {
      depths.set(name, 0)
      return 0
    }
    seen.add(name)
    const depth = Math.min(16, depthOf(device.parent, seen) + 1)
    depths.set(name, depth)
    return depth
  }

  return devices.map((device) => ({
    ...device,
    depth: depthOf(device.name, new Set()),
  }))
}

/** Partitions only, in the order lsblk reported them. */
export function partitionRows(devices: VolumeDevice[]): VolumeDevice[] {
  return devices.filter((device) => device.kind === 'part')
}

/** The whole disks the panel may edit, from `disk_allow`. */
export function allowedDisks(
  devices: VolumeDevice[],
  diskAllow: string[],
): VolumeDevice[] {
  return devices.filter(
    (device) => device.kind === 'disk' && diskAllow.includes(device.path),
  )
}

/** The partition number at the end of a kernel name. */
export function partitionNumber(name: string): number | undefined {
  const match = /(\d+)$/.exec(name)
  if (!match) return undefined
  const value = Number(match[1])
  return Number.isSafeInteger(value) && value > 0 ? value : undefined
}

/** Whether a partition belongs to a disk in the allowlist. */
export function partitionAllowed(
  device: VolumeDevice,
  diskAllow: string[],
): boolean {
  if (!device.parent) return false
  return diskAllow.some((path) => path.split('/').pop() === device.parent)
}

/** How many devices have at least one mountpoint. */
export function mountedCount(devices: VolumeDevice[]): number {
  return devices.filter((device) => device.mountpoints.length > 0).length
}

/** Whether a device path is an active swap area. */
export function isSwap(device: VolumeDevice, swapDevices: string[]): boolean {
  return swapDevices.includes(device.path) || swapDevices.includes(device.name)
}

/** Filters devices by name, path, label, model or mountpoint. */
export function filterDevices(rows: DeviceRow[], query: string): DeviceRow[] {
  const needle = query.trim().toLowerCase()
  if (needle === '') return rows
  return rows.filter((device) =>
    [
      device.name,
      device.path,
      device.kind,
      device.fstype ?? '',
      device.label ?? '',
      device.model ?? '',
      ...device.mountpoints,
    ]
      .join(' ')
      .toLowerCase()
      .includes(needle),
  )
}

/** Filters mounts by mountpoint, source, filesystem or options. */
export function filterMounts(mounts: VolumeMount[], query: string): VolumeMount[] {
  const needle = query.trim().toLowerCase()
  if (needle === '') return mounts
  return mounts.filter((mount) =>
    [mount.mountpoint, mount.source, mount.fstype, mount.options]
      .join(' ')
      .toLowerCase()
      .includes(needle),
  )
}

/** Used percentage for a mount, clamped for a bar. */
export function mountUsagePercent(usage: {
  total_bytes: number
  used_bytes: number
}): number {
  if (usage.total_bytes <= 0) return 0
  return Math.min(100, Math.max(0, (usage.used_bytes / usage.total_bytes) * 100))
}
