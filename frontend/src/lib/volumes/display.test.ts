import { describe, expect, it } from 'vitest'
import {
  availableLabel,
  compareDeviceRows,
  compareMounts,
  deviceCountLabel,
  deviceDepth,
  filterDeviceRows,
  filterMounts,
  kindLabel,
  mountableDevices,
  mountCountLabel,
  mountLabel,
  mountMode,
  toDeviceRows,
  usageLabel,
  usedPercent,
} from './display'
import { volumeDevice, volumeMount, volumeSnapshot } from '../../test/fake-volume-client'

const devices = [
  volumeDevice({
    name: 'sda',
    path: '/dev/sda',
    kind: 'disk',
    fstype: undefined,
    label: undefined,
    mountpoints: [],
    size_bytes: 80_000_000_000,
  }),
  volumeDevice({ parent: 'sda', size_bytes: 79_000_000_000 }),
  volumeDevice({
    name: 'sda2',
    path: '/dev/sda2',
    parent: 'sda',
    size_bytes: 1_000_000_000,
    fstype: 'xfs',
    mountpoints: [],
  }),
  volumeDevice({
    name: 'sdb1',
    path: '/dev/sdb1',
    parent: 'sdb',
    fstype: 'vfat',
    label: 'usb',
    mountpoints: [],
    removable: true,
    size_bytes: 16_000_000_000,
  }),
]

describe('deviceDepth', () => {
  it('resolves a parent chain', () => {
    expect(deviceDepth(devices, devices[0]!)).toBe(0)
    expect(deviceDepth(devices, devices[1]!)).toBe(1)
    expect(deviceDepth(devices, devices[3]!)).toBe(1)
  })

  it('stops when the parent is outside the list', () => {
    expect(deviceDepth([devices[2]!], devices[2]!)).toBe(1)
  })

  it('survives a parent cycle', () => {
    const cyclic = [
      volumeDevice({ name: 'a', path: '/dev/a', parent: 'b' }),
      volumeDevice({ name: 'b', path: '/dev/b', parent: 'a' }),
    ]
    expect(deviceDepth(cyclic, cyclic[0]!)).toBeLessThan(cyclic.length + 1)
  })
})

describe('filterDeviceRows', () => {
  it('returns everything for an empty filter', () => {
    const rows = toDeviceRows(devices)
    expect(filterDeviceRows(rows, '   ')).toEqual(rows)
  })

  it('matches the label, path, filesystem and mountpoint', () => {
    const rows = toDeviceRows(devices)
    expect(filterDeviceRows(rows, 'usb').map((row) => row.name)).toEqual([
      'sdb1',
    ])
    expect(filterDeviceRows(rows, '/dev/sda2').map((row) => row.name)).toEqual([
      'sda',
      'sda2',
    ])
    expect(filterDeviceRows(rows, 'ext4').map((row) => row.name)).toEqual([
      'sda',
      'sda1',
    ])
  })

  it('keeps ancestors so the tree stays readable', () => {
    const rows = toDeviceRows(devices)
    expect(filterDeviceRows(rows, 'ext4').map((row) => row.name)).toEqual([
      'sda',
      'sda1',
    ])
  })
})

describe('filterMounts', () => {
  const mounts = [
    volumeMount(),
    volumeMount({ mountpoint: '/proc', source: 'proc', fstype: 'proc' }),
  ]

  it('matches mountpoint, source and filesystem', () => {
    expect(filterMounts(mounts, 'proc').map((mount) => mount.mountpoint)).toEqual([
      '/proc',
    ])
    expect(filterMounts(mounts, '').length).toBe(2)
  })
})

describe('usage labels', () => {
  it('formats used and free space', () => {
    const mount = volumeMount()
    expect(usedPercent(mount)).toBeCloseTo(40)
    expect(usageLabel(mount)).toBe('400 B (40%)')
    expect(availableLabel(mount)).toBe('550 B')
  })

  it('renders an em dash without usage', () => {
    const mount = volumeMount({ usage: undefined })
    expect(usedPercent(mount)).toBeNull()
    expect(usageLabel(mount)).toBe('—')
    expect(availableLabel(mount)).toBe('—')
  })

  it('avoids dividing by a zero total', () => {
    const mount = volumeMount({
      usage: { total_bytes: 0, used_bytes: 0, available_bytes: 0 },
    })
    expect(usedPercent(mount)).toBeNull()
    expect(usageLabel(mount)).toBe('0 B')
  })
})

describe('mountMode and labels', () => {
  it('labels read-only and virtual mounts', () => {
    expect(mountMode(volumeMount())).toBe('rw')
    expect(mountMode(volumeMount({ read_only: true }))).toBe('ro')
    expect(mountMode(volumeMount({ virtual_fs: true }))).toBe('virtual')
    expect(mountMode(volumeMount({ virtual_fs: true, read_only: true }))).toBe(
      'ro, virtual',
    )
  })

  it('joins mountpoints and falls back to an em dash', () => {
    expect(mountLabel(volumeDevice({ mountpoints: ['/', '/mnt'] }))).toBe(
      '/, /mnt',
    )
    expect(mountLabel(volumeDevice({ mountpoints: [] }))).toBe('—')
  })

  it('survives a device with no mountpoints key at all', () => {
    // Older daemons omitted the key for an unmounted device. One such row used
    // to make every helper throw and take the whole table down with it.
    const serialized = JSON.parse(
      '{"name":"sdb1","path":"/dev/sdb1","kind":"part","size_bytes":1024,' +
        '"fstype":"vfat","read_only":false,"removable":true}',
    )
    const device = serialized as Parameters<typeof mountLabel>[0]

    expect(mountLabel(device)).toBe('—')
    expect(toDeviceRows([device])[0].mountLabel).toBe('—')
    expect(mountableDevices([device]).map((entry) => entry.name)).toEqual([
      'sdb1',
    ])
    expect(
      compareDeviceRows(
        toDeviceRows([device])[0],
        toDeviceRows([device])[0],
        'mount',
        'asc',
      ),
    ).toBe(0)
  })

  it('handles the daemon payload for an unmounted device', () => {
    // The shape the daemon actually sends: `mountpoints` present and empty.
    const device = JSON.parse(
      '{"name":"sdb1","path":"/dev/sdb1","kind":"part","size_bytes":1024,' +
        '"fstype":"vfat","mountpoints":[],"read_only":false,"removable":true}',
    ) as Parameters<typeof mountLabel>[0]

    expect(mountLabel(device)).toBe('—')
    expect(mountableDevices([device])).toHaveLength(1)
  })

  it('names the kind', () => {
    expect(kindLabel('part')).toBe('part')
    expect(kindLabel('raid')).toBe('raid')
  })
})

describe('mountableDevices', () => {
  it('offers only unmounted partitions with a filesystem', () => {
    expect(mountableDevices(devices).map((device) => device.name)).toEqual([
      'sda2',
      'sdb1',
    ])
  })
})

describe('sorting', () => {
  it('sorts devices by size in both directions', () => {
    const rows = toDeviceRows(devices)
    const asc = [...rows].sort((a, b) =>
      compareDeviceRows(a, b, 'size', 'asc'),
    )
    expect(asc.map((row) => row.name)).toEqual(['sda2', 'sdb1', 'sda1', 'sda'])
  })

  it('sorts devices by name with a stable tie-break', () => {
    const rows = toDeviceRows(devices)
    const desc = [...rows].sort((a, b) =>
      compareDeviceRows(a, b, 'device', 'desc'),
    )
    expect(desc.map((row) => row.name)).toEqual(['sdb1', 'sda2', 'sda1', 'sda'])
  })

  it('sorts mounts by used space', () => {
    const mounts = [
      volumeMount({
        mountpoint: '/small',
        usage: { total_bytes: 100, used_bytes: 10, available_bytes: 90 },
      }),
      volumeMount({
        mountpoint: '/big',
        usage: { total_bytes: 100, used_bytes: 90, available_bytes: 10 },
      }),
    ]
    const desc = [...mounts].sort((a, b) =>
      compareMounts(a, b, 'used', 'desc'),
    )
    expect(desc.map((mount) => mount.mountpoint)).toEqual(['/big', '/small'])
  })
})

describe('count labels', () => {
  it('reports shown and total when shortened', () => {
    const snapshot = volumeSnapshot({ total: 9000, truncated: true })
    expect(deviceCountLabel(snapshot)).toBe('4 of 9000 devices')
    expect(mountCountLabel(snapshot)).toBe('1 mount')
  })

  it('reports a plain count otherwise', () => {
    expect(deviceCountLabel(volumeSnapshot())).toBe('4 devices')
  })
})
