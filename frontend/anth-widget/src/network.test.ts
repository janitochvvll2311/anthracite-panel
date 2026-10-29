import { afterEach, expect, test, vi } from 'vitest'

import './network'
import { HttpNetworkClient } from 'anth-client'
import type {
  NetworkClient,
  NetworkHandlers,
  NetworkInterface,
  NetworkSnapshot,
} from 'anth-wire'

function iface(overrides: Partial<NetworkInterface> = {}): NetworkInterface {
  return {
    name: 'eth0',
    index: 2,
    kind: 'ether',
    mac: '00:11:22:33:44:55',
    mtu: 1500,
    state: 'up',
    up: true,
    running: true,
    loopback: false,
    physical: false,
    addresses: [
      { family: 'inet', address: '192.168.1.10', prefix: 24, scope: 'global' },
    ],
    rx_bytes: 1000,
    tx_bytes: 500,
    rx_packets: 10,
    tx_packets: 5,
    rx_errors: 0,
    tx_errors: 0,
    rx_dropped: 0,
    tx_dropped: 0,
    speed_mbps: 1000,
    ...overrides,
  }
}

function snapshot(overrides: Partial<NetworkSnapshot> = {}): NetworkSnapshot {
  return {
    interfaces: [
      iface(),
      iface({
        name: 'wlan0',
        index: 3,
        kind: 'wifi',
        state: 'down',
        up: false,
        mac: 'aa:bb:cc:dd:ee:ff',
        addresses: [],
        speed_mbps: null,
      }),
    ],
    routes: [{ family: 'inet', gateway: '192.168.1.1', interface: 'eth0' }],
    timestamp_ms: 1_000,
    control_enabled: false,
    manage_enabled: false,
    ...overrides,
  }
}

/** Three interfaces with both policy switches on: eth0, wlan0 and lo. */
function managedSnapshot(): NetworkSnapshot {
  return snapshot({
    control_enabled: true,
    manage_enabled: true,
    interfaces: [
      iface(),
      iface({
        name: 'wlan0',
        index: 3,
        kind: 'wifi',
        state: 'down',
        up: false,
        running: false,
        mac: 'aa:bb:cc:dd:ee:ff',
        addresses: [],
        speed_mbps: null,
      }),
      iface({
        name: 'lo',
        index: 1,
        kind: 'loopback',
        loopback: true,
        mac: '',
        addresses: [
          { family: 'inet', address: '127.0.0.1', prefix: 8, scope: 'host' },
        ],
        speed_mbps: null,
      }),
    ],
  })
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

type NetworkElement = HTMLElement & {
  client: NetworkClient
  updateComplete: Promise<boolean>
}

function fakeClient() {
  let handlers: NetworkHandlers | undefined
  const close = vi.fn()
  const watch = vi.fn((_interval: number, next: NetworkHandlers) => {
    handlers = next
    next.onStatus?.('live')
    return close
  })
  const get = vi.fn().mockResolvedValue(snapshot())
  const setState = vi.fn().mockResolvedValue('`eth0` is down')
  const create = vi.fn().mockResolvedValue('created dummy interface `dummy0`')
  const edit = vi.fn().mockResolvedValue('updated `eth0`: mtu 1400')
  const rename = vi.fn().mockResolvedValue('renamed `eth0` to `lan0`')
  const remove = vi.fn().mockResolvedValue('deleted interface `eth0`')
  const client: NetworkClient = { get, watch, setState, create, edit, rename, remove }
  return {
    client,
    watch,
    close,
    get,
    setState,
    create,
    edit,
    rename,
    remove,
    push: (next: NetworkSnapshot) => handlers?.onSnapshot(next),
    fail: (error: Error) => handlers?.onError(error),
  }
}

async function mount(client: NetworkClient): Promise<NetworkElement> {
  const element = document.createElement('anth-network') as NetworkElement
  element.client = client
  document.body.append(element)
  await element.updateComplete
  await element.updateComplete
  return element
}

async function settle(element: NetworkElement): Promise<void> {
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

test('renders interfaces, addresses and derived rates', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const text = element.shadowRoot?.textContent ?? ''
  expect(text).toContain('eth0')
  expect(text).toContain('wlan0')
  expect(text).toContain('192.168.1.10/24')
  expect(text).toContain('192.168.1.1')
  expect(text).toContain('2 interfaces · 1 up')
  expect(text).toContain('1 Gbps')
  expect(text).toContain('—')

  feed.push(
    snapshot({
      timestamp_ms: 3_000,
      interfaces: [iface({ rx_bytes: 5_096, tx_bytes: 1_524 })],
    }),
  )
  await element.updateComplete

  const updated = element.shadowRoot?.textContent ?? ''
  expect(updated).toContain('2.0 KiB/s')
  expect(updated).toContain('512 B/s')
})

test('filters the visible interfaces and keeps the count of the whole set', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const input = element.shadowRoot?.querySelector<HTMLInputElement>(
    'input[type="search"]',
  )!
  input.value = 'wlan'
  input.dispatchEvent(new Event('input'))
  await element.updateComplete

  const cards = element.shadowRoot?.querySelectorAll<HTMLElement>('.iface') ?? []
  expect(cards).toHaveLength(1)
  expect(cards[0].textContent).toContain('wlan0')
  expect(element.shadowRoot?.textContent).toContain('1 of 2 interfaces')
})

test('an empty inventory has its own sentence, distinct from an empty filter', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot({ interfaces: [], routes: [] }))
  await element.updateComplete

  const text = element.shadowRoot?.textContent ?? ''
  expect(text).toContain('No interfaces found.')
  expect(text).not.toContain('No interfaces match the current filter.')
  expect(text).toContain('0 interfaces · 0 up')
})

test('pausing closes the feed, resuming opens it again and an error is shown', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const pause = element.shadowRoot?.querySelector<HTMLButtonElement>('.toolbar button')!
  pause.click()
  await element.updateComplete
  expect(feed.close).toHaveBeenCalledTimes(1)
  expect(element.shadowRoot?.textContent).toContain('paused')

  pause.click()
  await element.updateComplete
  expect(feed.watch).toHaveBeenCalledTimes(2)
  expect(element.shadowRoot?.textContent).toContain('live')

  feed.fail(new Error('network is down'))
  await element.updateComplete
  expect(element.shadowRoot?.textContent).toContain('network is down')
})

test('a missing client renders only the empty state', async () => {
  const element = document.createElement('anth-network') as NetworkElement
  document.body.append(element)
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.toolbar')).toBeNull()
  expect(element.shadowRoot?.textContent).toContain('No network information yet.')
})

test('mutation controls appear only when the daemon allows them', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.manage')).toBeNull()
  expect(element.shadowRoot?.textContent).not.toContain('Create')

  feed.push(snapshot({ control_enabled: true, manage_enabled: true }))
  await element.updateComplete
  expect(element.shadowRoot?.querySelectorAll('.manage')).toHaveLength(2)
  expect(element.shadowRoot?.textContent).toContain('Create')
})

test('up/down, edit, rename and delete go through the client', async () => {
  const feed = fakeClient()
  feed.get.mockResolvedValue(managedSnapshot())
  const element = await mount(feed.client)
  feed.push(managedSnapshot())
  await element.updateComplete

  const cards = () =>
    [...(element.shadowRoot?.querySelectorAll<HTMLElement>('.iface') ?? [])]
  const named = (name: string) =>
    cards().find((card) => card.querySelector('h3')?.textContent === name)!
  const eth0 = () => named('eth0')
  const wlan0 = () => named('wlan0')

  // Loopback cannot go down and cannot be deleted.
  const loopback = named('lo')
  expect(button(loopback, 'Down').disabled).toBe(true)
  expect(button(loopback, 'Delete').disabled).toBe(true)

  // The only default-route carrier cannot be deleted either.
  expect(button(eth0(), 'Delete').disabled).toBe(true)

  button(eth0(), 'Down').click()
  await settle(element)
  expect(feed.setState).toHaveBeenCalledWith('eth0', false)
  expect(feed.get).toHaveBeenCalled()
  expect(element.shadowRoot?.textContent).toContain('Change applied.')

  button(eth0(), 'Edit MTU').click()
  await element.updateComplete
  const mtuForm = eth0().querySelector<HTMLFormElement>('form.inline')!
  mtuForm.querySelector<HTMLInputElement>('input[name="mtu"]')!.value = '1400'
  mtuForm.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)
  expect(feed.edit).toHaveBeenCalledWith('eth0', { mtu: 1400 })
  expect(eth0().querySelector('form.inline')).toBeNull()

  // wlan0 is down, so it can be renamed and deleted.
  button(wlan0(), 'Rename').click()
  await element.updateComplete
  const renameForm = wlan0().querySelector<HTMLFormElement>('form.inline')!
  renameForm.querySelector<HTMLInputElement>('input[name="new_name"]')!.value = 'wlan1'
  renameForm.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)
  expect(feed.rename).toHaveBeenCalledWith('wlan0', 'wlan1')

  button(wlan0(), 'Delete').click()
  await element.updateComplete
  expect(wlan0().querySelector('.confirm')?.textContent).toContain('This cannot be undone.')
  button(wlan0().querySelector('.confirm')!, 'Delete').click()
  await settle(element)
  expect(feed.remove).toHaveBeenCalledWith('wlan0')
})

test('the create form posts the selected virtual interface', async () => {
  const feed = fakeClient()
  feed.get.mockResolvedValue(snapshot({ control_enabled: true, manage_enabled: true }))
  const element = await mount(feed.client)
  feed.push(snapshot({ control_enabled: true, manage_enabled: true }))
  await element.updateComplete

  button(element.shadowRoot!, 'Create').click()
  await element.updateComplete
  const form = element.shadowRoot?.querySelector<HTMLFormElement>('form.create')!
  const name = form.querySelector<HTMLInputElement>('input[name="name"]')!
  name.value = 'dummy0'
  const kind = form.querySelector<HTMLSelectElement>('select[name="kind"]')!
  kind.value = 'dummy'
  kind.dispatchEvent(new Event('change'))
  await element.updateComplete
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.create).toHaveBeenCalledWith({
    name: 'dummy0',
    kind: 'dummy',
    up: false,
  })
  expect(element.shadowRoot?.querySelector('form.create')).toBeNull()
})

test('a failed mutation keeps the listing and shows the daemon message', async () => {
  const feed = fakeClient()
  feed.setState.mockRejectedValueOnce(new Error('operation not permitted'))
  feed.get.mockResolvedValue(snapshot({ control_enabled: true, manage_enabled: true }))
  const element = await mount(feed.client)
  feed.push(snapshot({ control_enabled: true, manage_enabled: true }))
  await element.updateComplete

  const eth0 = [...(element.shadowRoot?.querySelectorAll<HTMLElement>('.iface') ?? [])].find(
    (card) => card.querySelector('h3')?.textContent === 'eth0',
  )!
  button(eth0, 'Down').click()
  await settle(element)

  expect(element.shadowRoot?.textContent).toContain('operation not permitted')
  expect(element.shadowRoot?.querySelectorAll('.iface')).toHaveLength(2)
})

test('the HTTP client reads /api/network and surfaces the daemon error', async () => {
  const calls: string[] = []
  const fakeFetch = vi.fn((url: string) => {
    calls.push(url)
    return Promise.resolve(new Response(JSON.stringify(snapshot())))
  }) as unknown as typeof fetch
  const client = new HttpNetworkClient(fakeFetch)
  await expect(client.get()).resolves.toMatchObject({ timestamp_ms: 1_000 })
  expect(calls).toEqual(['/api/network'])

  const failing = new HttpNetworkClient((() =>
    Promise.resolve(
      new Response(
        JSON.stringify({
          error: { kind: 'internal', code: 'internal', message: 'no network' },
        }),
        { status: 500 },
      ),
    )) as unknown as typeof fetch)
  await expect(failing.get()).rejects.toThrow('no network')
})

test('the HTTP client sends the mutation bodies the daemon expects', async () => {
  const calls: Array<{ url: string; init?: RequestInit }> = []
  const fakeFetch = vi.fn((url: string, init?: RequestInit) => {
    calls.push({ url, init })
    return Promise.resolve(new Response(JSON.stringify({ message: 'ok' })))
  }) as unknown as typeof fetch
  const client = new HttpNetworkClient(fakeFetch)

  await expect(client.setState('eth0', false)).resolves.toBe('ok')
  await expect(
    client.create({ name: 'v0', kind: 'veth', peer: 'v1', mtu: 1400, up: true }),
  ).resolves.toBe('ok')
  await expect(client.edit('eth0', { mtu: 9000, up: false })).resolves.toBe('ok')
  await expect(client.rename('eth0', 'lan0')).resolves.toBe('ok')
  await expect(client.remove('eth0')).resolves.toBe('ok')

  expect(calls.map((call) => call.url)).toEqual([
    '/api/network/state',
    '/api/network/create',
    '/api/network/edit',
    '/api/network/rename',
    '/api/network/delete',
  ])
  const bodies = calls.map((call) => JSON.parse(String(call.init?.body)))
  expect(bodies).toEqual([
    { name: 'eth0', up: false },
    { name: 'v0', kind: 'veth', peer: 'v1', mtu: 1400, up: true },
    { name: 'eth0', mtu: 9000, up: false },
    { name: 'eth0', new_name: 'lan0' },
    { name: 'eth0' },
  ])
  expect(calls.every((call) => call.init?.method === 'POST')).toBe(true)
})
