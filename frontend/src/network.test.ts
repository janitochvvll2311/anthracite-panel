import { afterEach, expect, test, vi } from 'vitest'

import './network'
import { HttpNetworkClient } from './network-client'
import type {
  NetworkClient,
  NetworkHandlers,
  NetworkInterface,
  NetworkSnapshot,
} from './network-client'

function iface(overrides: Partial<NetworkInterface> = {}): NetworkInterface {
  return {
    name: 'eth0',
    index: 2,
    kind: 'ether',
    mac: '00:11:22:33:44:55',
    mtu: 1500,
    state: 'up',
    up: true,
    loopback: false,
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
  const client: NetworkClient = { get, watch }
  return {
    client,
    watch,
    close,
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
