import { expect, test } from 'vitest'

import {
  deviceRows,
  filterDevices,
  filterMounts,
  isSwap,
  mountUsagePercent,
  mountedCount,
  partitionRows,
} from './lib/storage/display'
import type { VolumeDevice, VolumeMount } from './storage-client'

function device(overrides: Partial<VolumeDevice> = {}): VolumeDevice {
  return {
    name: 'sda',
    path: '/dev/sda',
    kind: 'disk',
    size_bytes: 1000,
    mountpoints: [],
    read_only: false,
    removable: false,
    ...overrides,
  }
}

function mount(overrides: Partial<VolumeMount> = {}): VolumeMount {
  return {
    mountpoint: '/',
    source: '/dev/sda1',
    fstype: 'ext4',
    options: 'rw,relatime',
    read_only: false,
    virtual_fs: false,
    ...overrides,
  }
}

test('device depth follows parent links in the lsblk order', () => {
  const rows = deviceRows([
    device(),
    device({ name: 'sda1', path: '/dev/sda1', kind: 'part', parent: 'sda' }),
    device({ name: 'sda2', path: '/dev/sda2', kind: 'part', parent: 'sda' }),
    device({ name: 'loop0', path: '/dev/loop0', kind: 'loop' }),
  ])
  expect(rows.map((row) => row.depth)).toEqual([0, 1, 1, 0])
  // A missing parent degrades to a root row instead of a broken tree.
  expect(deviceRows([device({ name: 'x', parent: 'missing' })])[0].depth).toBe(1)
})

test('partitions, mounted counts and swap use the flattened list', () => {
  const devices = [
    device(),
    device({ name: 'sda1', path: '/dev/sda1', kind: 'part', parent: 'sda', mountpoints: ['/'] }),
    device({ name: 'sda2', path: '/dev/sda2', kind: 'part', parent: 'sda' }),
    device({ name: 'loop0', kind: 'loop' }),
  ]
  expect(partitionRows(devices).map((row) => row.name)).toEqual(['sda1', 'sda2'])
  expect(mountedCount(devices)).toBe(1)
  expect(isSwap(devices[2], ['/dev/sda2'])).toBe(true)
  expect(isSwap(devices[2], ['sda2'])).toBe(true)
  expect(isSwap(devices[1], ['/dev/sda2'])).toBe(false)
})

test('filters match device metadata and mount sources', () => {
  const rows = deviceRows([
    device({ label: 'root', model: 'Example Disk' }),
    device({ name: 'sdb', path: '/dev/sdb', kind: 'disk', label: 'backup' }),
  ])
  expect(filterDevices(rows, '')).toHaveLength(2)
  expect(filterDevices(rows, 'root').map((row) => row.name)).toEqual(['sda'])
  expect(filterDevices(rows, '/dev/sdb').map((row) => row.name)).toEqual(['sdb'])
  expect(filterDevices(rows, 'missing')).toHaveLength(0)

  const mounts = [
    mount(),
    mount({ mountpoint: '/proc', source: 'proc', fstype: 'proc', virtual_fs: true }),
  ]
  expect(filterMounts(mounts, '/proc')).toHaveLength(1)
  expect(filterMounts(mounts, 'ext4')).toHaveLength(1)
  expect(filterMounts(mounts, 'nope')).toHaveLength(0)
})

test('usage percent is clamped and safe with an empty filesystem', () => {
  expect(mountUsagePercent({ total_bytes: 100, used_bytes: 40 })).toBe(40)
  expect(mountUsagePercent({ total_bytes: 0, used_bytes: 10 })).toBe(0)
  expect(mountUsagePercent({ total_bytes: 100, used_bytes: 150 })).toBe(100)
})
