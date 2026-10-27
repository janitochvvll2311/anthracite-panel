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
      mount({
        mountpoint: '/mnt/data',
        source: '/dev/sda1',
        fstype: 'ext4',
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

function button(container: ParentNode, label: string): HTMLButtonElement {
  const found = [...container.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!found) throw new Error(`missing button: ${label}`)
  return found
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
  const mount = vi.fn().mockResolvedValue('mounted /dev/sdb1 on /mnt/data as ext4')
  const unmount = vi.fn().mockResolvedValue('unmounted /mnt/data')
  const createPartition = vi.fn().mockResolvedValue('created partition')
  const resizePartition = vi.fn().mockResolvedValue('resized partition')
  const deletePartition = vi.fn().mockResolvedValue('deleted partition')
  const formatPartition = vi.fn().mockResolvedValue('formatted partition')
  return {
    client: {
      get,
      mount,
      unmount,
      createPartition,
      resizePartition,
      deletePartition,
      formatPartition,
    } as StorageClient,
    get,
    mount,
    unmount,
    createPartition,
    resizePartition,
    deletePartition,
    formatPartition,
  }
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
  expect(element.shadowRoot?.querySelectorAll('.mount')).toHaveLength(3)
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

test('mounts an unmounted device through the form', async () => {
  const feed = fakeClient()
  const element = await mountStorage(feed.client)
  const form = element.shadowRoot?.querySelector<HTMLFormElement>('form.mount-form')!
  expect(form.querySelector<HTMLSelectElement>('select[name="device"]')?.value).toBe('/dev/sda')

  const target = form.querySelector<HTMLInputElement>('input[name="target"]')!
  target.value = '/mnt/data'
  target.dispatchEvent(new Event('input'))
  const fsType = form.querySelector<HTMLInputElement>('input[name="fs_type"]')!
  fsType.value = 'ext4'
  fsType.dispatchEvent(new Event('input'))
  const readOnly = form.querySelector<HTMLInputElement>('input[name="read_only"]')!
  readOnly.checked = true
  readOnly.dispatchEvent(new Event('change'))
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.mount).toHaveBeenCalledWith({
    device: '/dev/sda',
    target: '/mnt/data',
    fs_type: 'ext4',
    read_only: true,
    allow_removable: false,
  })
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
  expect(form.querySelector<HTMLInputElement>('input[name="target"]')?.value).toBe('')
})

test('unmounts a non-virtual mount after confirmation', async () => {
  const feed = fakeClient()
  const element = await mountStorage(feed.client)
  const row = [...(element.shadowRoot?.querySelectorAll<HTMLElement>('.mount') ?? [])].find(
    (candidate) => candidate.textContent?.includes('/mnt/data'),
  )!
  expect(row.querySelector('button.unmount')).not.toBeNull()
  row.querySelector<HTMLButtonElement>('button.unmount')!.click()
  await element.updateComplete
  expect(row.querySelector('.confirm')?.textContent).toContain('Unmount /mnt/data?')
  row.querySelector<HTMLButtonElement>('.confirm .danger')!.click()
  await settle(element)
  expect(feed.unmount).toHaveBeenCalledWith('/mnt/data')
})

test('control off hides the mount form and the unmount button', async () => {
  const feed = fakeClient(snapshot({ control_enabled: false }))
  const element = await mountStorage(feed.client)
  expect(element.shadowRoot?.querySelector('form.mount-form')).toBeNull()
  expect(element.shadowRoot?.querySelector('button.unmount')).toBeNull()
})

test('creates a partition on an allowed disk', async () => {
  const feed = fakeClient(
    snapshot({ partition_enabled: true, format_enabled: true }),
  )
  const element = await mountStorage(feed.client, 'partitions')
  const form = element.shadowRoot?.querySelector<HTMLFormElement>('.partition-create')!
  const size = form.querySelector<HTMLInputElement>('input[name="size_mib"]')!
  size.value = '16'
  size.dispatchEvent(new Event('input'))
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.createPartition).toHaveBeenCalledWith({
    disk: '/dev/sda',
    kind: 'linux',
    size_bytes: 16 * 1024 * 1024,
  })
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
})

test('resizes, formats and deletes a partition with confirmations', async () => {
  const feed = fakeClient(
    snapshot({ partition_enabled: true, format_enabled: true }),
  )
  const element = await mountStorage(feed.client, 'partitions')
  const card = () =>
    [...(element.shadowRoot?.querySelectorAll<HTMLElement>('.partition') ?? [])].find(
      (candidate) => candidate.querySelector('h3')?.textContent === 'sda1',
    )!
  const wait = async () => {
    await element.updateComplete
  }

  button(card(), 'Resize').click()
  await wait()
  const resize = card().querySelector<HTMLFormElement>('.partition-inline')!
  const resizeSize = resize.querySelector<HTMLInputElement>('input[name="size_mib"]')!
  resizeSize.value = '32'
  resize.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)
  expect(feed.resizePartition).toHaveBeenCalledWith({
    disk: '/dev/sda',
    number: 1,
    size_bytes: 32 * 1024 * 1024,
  })

  button(card(), 'Format').click()
  await wait()
  const format = card().querySelector<HTMLFormElement>('.partition-inline')!
  const label = format.querySelector<HTMLInputElement>('input[name="label"]')!
  label.value = 'root'
  format.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)
  expect(feed.formatPartition).toHaveBeenCalledWith({
    partition: '/dev/sda1',
    fs_type: 'ext4',
    label: 'root',
  })

  button(card(), 'Delete').click()
  await wait()
  expect(card().querySelector('.confirm')?.textContent).toContain('Delete partition sda1?')
  button(card().querySelector('.confirm')!, 'Delete').click()
  await settle(element)
  expect(feed.deletePartition).toHaveBeenCalledWith({
    disk: '/dev/sda',
    number: 1,
  })
})

test('partition actions are absent when their policy is off', async () => {
  const feed = fakeClient(
    snapshot({ partition_enabled: false, format_enabled: false }),
  )
  const element = await mountStorage(feed.client, 'partitions')
  expect(element.shadowRoot?.querySelector('.partition-create')).toBeNull()
  expect(element.shadowRoot?.querySelector('.partition .manage')).toBeNull()
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

test('the HTTP client sends mount and unmount bodies', async () => {
  const calls: Array<{ url: string; init?: RequestInit }> = []
  const fakeFetch = vi.fn((url: string, init?: RequestInit) => {
    calls.push({ url, init })
    return Promise.resolve(new Response(JSON.stringify({ message: 'ok' })))
  }) as unknown as typeof fetch
  const client = new HttpStorageClient(fakeFetch)

  await expect(
    client.mount({
      device: '/dev/sdb1',
      target: '/mnt/data',
      fs_type: 'ext4',
      read_only: true,
      allow_removable: false,
    }),
  ).resolves.toBe('ok')
  await expect(client.unmount('/mnt/data')).resolves.toBe('ok')
  await expect(
    client.createPartition({ disk: '/dev/sdb', kind: 'linux', size_bytes: 1024 }),
  ).resolves.toBe('ok')
  await expect(
    client.resizePartition({ disk: '/dev/sdb', number: 1, size_bytes: 2048 }),
  ).resolves.toBe('ok')
  await expect(
    client.deletePartition({ disk: '/dev/sdb', number: 1 }),
  ).resolves.toBe('ok')
  await expect(
    client.formatPartition({ partition: '/dev/sdb1', fs_type: 'ext4' }),
  ).resolves.toBe('ok')

  expect(calls.map((call) => call.url)).toEqual([
    '/api/volumes/mount',
    '/api/volumes/unmount',
    '/api/volumes/partition/create',
    '/api/volumes/partition/resize',
    '/api/volumes/partition/delete',
    '/api/volumes/format',
  ])
  expect(calls.map((call) => JSON.parse(String(call.init?.body)))).toEqual([
    {
      device: '/dev/sdb1',
      target: '/mnt/data',
      fs_type: 'ext4',
      read_only: true,
      allow_removable: false,
    },
    { target: '/mnt/data' },
    { disk: '/dev/sdb', kind: 'linux', size_bytes: 1024 },
    { disk: '/dev/sdb', number: 1, size_bytes: 2048 },
    { disk: '/dev/sdb', number: 1 },
    { partition: '/dev/sdb1', fs_type: 'ext4' },
  ])
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
