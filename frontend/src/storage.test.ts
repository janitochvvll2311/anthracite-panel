import { afterEach, expect, test, vi } from 'vitest'

import './storage'
import { HttpStorageClient } from './storage-client'
import type { StorageClient, VolumeDevice, VolumeMount, VolumeSnapshot } from './storage-client'

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
    usage: { total_bytes: 100, used_bytes: 40, available_bytes: 60 },
    ...overrides,
  }
}

function snapshot(overrides: Partial<VolumeSnapshot> = {}): VolumeSnapshot {
  return {
    devices: [
      device({ label: 'Example Disk', model: 'QEMU HARDDISK' }),
      device({
        name: 'sda1',
        path: '/dev/sda1',
        kind: 'part',
        parent: 'sda',
        size_bytes: 500,
        fstype: 'ext4',
        label: 'root',
        mountpoints: ['/'],
      }),
      device({
        name: 'sda2',
        path: '/dev/sda2',
        kind: 'part',
        parent: 'sda',
        size_bytes: 200,
        fstype: 'swap',
        mountpoints: [],
      }),
      device({ name: 'loop0', path: '/dev/loop0', kind: 'loop', size_bytes: 10, read_only: true }),
    ],
    mounts: [
      mount(),
      mount({
        mountpoint: '/proc',
        source: 'proc',
        fstype: 'proc',
        virtual_fs: true,
        usage: undefined,
      }),
    ],
    swap_devices: ['/dev/sda2'],
    control_enabled: true,
    partition_enabled: false,
    format_enabled: false,
    mount_root: '/mnt',
    disk_allow: ['/dev/sda'],
    timestamp_ms: 1,
    ...overrides,
  }
}

type StorageElement = HTMLElement & {
  client: StorageClient
  view: 'volumes' | 'partitions'
  updateComplete: Promise<boolean>
}

async function settle(element: StorageElement): Promise<void> {
  for (let index = 0; index < 6; index += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mountStorage(
  client: StorageClient,
  view: 'volumes' | 'partitions' = 'volumes',
): Promise<StorageElement> {
  const element = document.createElement('anth-storage') as StorageElement
  element.client = client
  element.view = view
  document.body.append(element)
  await settle(element)
  return element
}

function fakeClient(value = snapshot()) {
  const get = vi.fn().mockResolvedValue(value)
  return { client: { get } as StorageClient, get }
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

test('renders devices, mountpoints, mounts and swap', async () => {
  const feed = fakeClient()
  const element = await mountStorage(feed.client)
  const text = element.shadowRoot?.textContent ?? ''

  expect(text).toContain('4 devices · 1 mounted')
  expect(text).toContain('sda')
  expect(text).toContain('sda1')
  expect(text).toContain('Example Disk')
  expect(text).toContain('1000 B')
  expect(text).toContain('/dev/sda1')
  expect(text).toContain('Mounts')
  expect(text).toContain('40 B / 100 B')
  expect(text).toContain('Swap')
  expect(text).toContain('/dev/sda2')
  expect(element.shadowRoot?.querySelectorAll('.device')).toHaveLength(4)
  expect(element.shadowRoot?.querySelectorAll('.mount')).toHaveLength(2)
})

test('partitions view filters the same inventory to partitions', async () => {
  const feed = fakeClient()
  const element = await mountStorage(feed.client, 'partitions')
  const text = element.shadowRoot?.textContent ?? ''

  expect(text).toContain('2 partitions')
  expect(text).toContain('/dev/sda')
  expect(element.shadowRoot?.querySelectorAll('.partition')).toHaveLength(2)
  expect(text).not.toContain('loop0')
})

test('the filter applies to devices and mounts independently', async () => {
  const feed = fakeClient()
  const element = await mountStorage(feed.client)
  const input = element.shadowRoot?.querySelector<HTMLInputElement>(
    'input[type="search"]',
  )!
  input.value = '/proc'
  input.dispatchEvent(new Event('input'))
  await element.updateComplete

  expect(element.shadowRoot?.querySelectorAll('.device')).toHaveLength(0)
  expect(element.shadowRoot?.querySelectorAll('.mount')).toHaveLength(1)
  expect(element.shadowRoot?.textContent).toContain('/proc')
})

test('a warning is shown without hiding the inventory', async () => {
  const feed = fakeClient(snapshot({ warning: 'device inventory unavailable: lsblk is not installed' }))
  const element = await mountStorage(feed.client)
  expect(element.shadowRoot?.textContent).toContain('lsblk is not installed')
  expect(element.shadowRoot?.querySelectorAll('.device')).toHaveLength(4)
})

test('an empty inventory has its own sentence', async () => {
  const feed = fakeClient(
    snapshot({ devices: [], mounts: [], swap_devices: [], warning: undefined }),
  )
  const element = await mountStorage(feed.client)
  const text = element.shadowRoot?.textContent ?? ''
  expect(text).toContain('No block devices found.')
  expect(text).toContain('No mounts found.')
})

test('a failed read keeps the error visible', async () => {
  const feed = fakeClient()
  feed.get.mockRejectedValue(new Error('storage is unavailable'))
  const element = await mountStorage(feed.client)
  expect(element.shadowRoot?.textContent).toContain('storage is unavailable')
})

test('the HTTP client reads /api/volumes and surfaces errors', async () => {
  const calls: string[] = []
  const fakeFetch = vi.fn((url: string) => {
    calls.push(url)
    return Promise.resolve(new Response(JSON.stringify(snapshot())))
  }) as unknown as typeof fetch
  const client = new HttpStorageClient(fakeFetch)
  await expect(client.get()).resolves.toMatchObject({ mount_root: '/mnt' })
  expect(calls).toEqual(['/api/volumes'])

  const failing = new HttpStorageClient((() =>
    Promise.resolve(
      new Response(
        JSON.stringify({
          error: { kind: 'internal', code: 'volumes_failed', message: 'no storage' },
        }),
        { status: 500 },
      ),
    )) as unknown as typeof fetch)
  await expect(failing.get()).rejects.toThrow('no storage')
})
