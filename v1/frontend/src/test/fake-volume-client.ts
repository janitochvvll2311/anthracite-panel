import { vi } from 'vitest'
import type {
  VolumeClient,
  VolumeDevice,
  VolumeMountpoint,
  VolumeMountRequest,
  VolumeSnapshot,
} from '../lib/volumes/volume-client'

export function volumeDevice(overrides: Partial<VolumeDevice> = {}): VolumeDevice {
  return {
    name: 'sda1',
    path: '/dev/sda1',
    kind: 'part',
    size_bytes: 512_000_000,
    fstype: 'ext4',
    label: 'root',
    mountpoints: ['/'],
    read_only: false,
    removable: false,
    ...overrides,
  }
}

export function volumeMount(
  overrides: Partial<VolumeMountpoint> = {},
): VolumeMountpoint {
  return {
    mountpoint: '/',
    source: '/dev/sda1',
    fstype: 'ext4',
    options: 'rw,relatime',
    read_only: false,
    virtual_fs: false,
    usage: {
      total_bytes: 1000,
      used_bytes: 400,
      available_bytes: 550,
    },
    ...overrides,
  }
}

export function volumeSnapshot(
  overrides: Partial<VolumeSnapshot> = {},
): VolumeSnapshot {
  return {
    devices: [
      volumeDevice({
        name: 'sda',
        path: '/dev/sda',
        kind: 'disk',
        size_bytes: 80_000_000_000,
        fstype: undefined,
        label: undefined,
        mountpoints: [],
      }),
      volumeDevice({ parent: 'sda', size_bytes: 79_000_000_000 }),
      volumeDevice({
        name: 'sdb1',
        path: '/dev/sdb1',
        parent: 'sdb',
        size_bytes: 16_000_000_000,
        fstype: 'vfat',
        label: 'usb',
        mountpoints: [],
        removable: true,
      }),
      volumeDevice({
        name: 'sdc1',
        path: '/dev/sdc1',
        parent: 'sdc',
        size_bytes: 8_000_000_000,
        fstype: 'xfs',
        label: 'srv',
        mountpoints: [],
      }),
    ],
    mounts: [volumeMount()],
    control_enabled: true,
    timestamp_ms: 1_700_000_000_000,
    total: 4,
    truncated: false,
    ...overrides,
  }
}

export class FakeVolumeClient implements VolumeClient {
  current = volumeSnapshot()
  getError?: Error
  mountError?: Error
  unmountError?: Error
  mountCalls: VolumeMountRequest[] = []
  unmountCalls: string[] = []

  get = vi.fn(async (): Promise<VolumeSnapshot> => {
    if (this.getError) throw this.getError
    return this.current
  })

  mount = vi.fn(async (request: VolumeMountRequest): Promise<string> => {
    if (this.mountError) throw this.mountError
    this.mountCalls.push(request)
    return `mounted ${request.device} on ${request.target}`
  })

  unmount = vi.fn(async (target: string): Promise<string> => {
    if (this.unmountError) throw this.unmountError
    this.unmountCalls.push(target)
    return `unmounted ${target}`
  })

  emit(snapshot: VolumeSnapshot): void {
    this.current = snapshot
  }
}
