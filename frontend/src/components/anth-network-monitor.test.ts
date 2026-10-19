import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-network-monitor'
import type { AnthNetworkMonitor } from './anth-network-monitor'
import { ANTH_NETWORK_STATE_EVENT } from './anth-network-monitor'
import {
  FakeNetworkClient,
  networkInterface,
  networkSnapshot,
} from '../test/fake-network-client'

async function flush(element: AnthNetworkMonitor): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  client: FakeNetworkClient,
  options: { autoStart?: boolean; refreshInterval?: number } = {},
): Promise<AnthNetworkMonitor> {
  const element = document.createElement(
    'anth-network-monitor',
  ) as AnthNetworkMonitor
  element.client = client
  if (options.autoStart !== undefined) element.autoStart = options.autoStart
  if (options.refreshInterval !== undefined) {
    element.refreshInterval = options.refreshInterval
  }
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

function findButton(
  element: AnthNetworkMonitor,
  label: string,
): HTMLButtonElement | undefined {
  return [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
}

function button(element: AnthNetworkMonitor, label: string): HTMLButtonElement {
  const match = findButton(element, label)
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

function rows(element: AnthNetworkMonitor): HTMLTableRowElement[] {
  return [
    ...(element.shadowRoot?.querySelectorAll<HTMLTableRowElement>('tbody tr') ??
      []),
  ]
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('AnthNetworkMonitor', () => {
  it('loads a snapshot and renders interfaces, addresses and totals', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    const root = element.shadowRoot

    expect(client.watchCalls).toEqual([2000])
    expect(client.list).toHaveBeenCalledTimes(1)
    expect(root?.textContent).toContain('eth0')
    expect(root?.textContent).toContain('192.0.2.10/24')
    expect(root?.textContent).toContain('2.0 KiB')
    expect(root?.textContent).toContain('1.0 KiB')
    expect(root?.querySelectorAll('tbody tr')).toHaveLength(1)
    expect(root?.querySelector('.status')?.getAttribute('data-status')).toBe(
      'connecting',
    )
  })

  it('shows a truncation warning from the daemon', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([networkInterface()], {
      total: 3,
      truncated: true,
      warning: 'showing 1 of 3 interfaces',
    })
    const element = await mount(client)

    expect(
      element.shadowRoot?.querySelector('.warning')?.textContent,
    ).toContain('showing 1 of 3 interfaces')
  })

  it('computes rates from consecutive snapshots', async () => {
    const client = new FakeNetworkClient()
    const first = networkSnapshot()
    client.current = first
    const element = await mount(client)

    expect(rows(element)[0]?.textContent).toContain('—')

    client.emit(
      networkSnapshot(
        [networkInterface({ rx_bytes: 6144, tx_bytes: 3072 })],
        { timestamp_ms: first.timestamp_ms + 2000 },
      ),
    )
    await element.updateComplete

    expect(rows(element)[0]?.textContent).toContain('2.0 KiB/s')
    expect(rows(element)[0]?.textContent).toContain('1.0 KiB/s')
  })

  it('follows live status transitions and stream errors', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)

    client.status('live')
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.status')?.getAttribute('data-status')).toBe(
      'live',
    )

    client.fail(new Error('stream failed'))
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'stream failed',
    )
  })

  it('filters and sorts the table', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([
      networkInterface(),
      networkInterface({
        name: 'lo',
        index: 1,
        state: 'unknown',
        mac: '00:00:00:00:00:00',
        addresses: [{ family: 'ipv4', address: '127.0.0.1', prefix_len: 8 }],
        rx_bytes: 4096,
      }),
    ])
    const element = await mount(client)

    const filter = element.shadowRoot?.querySelector<HTMLInputElement>(
      'input[type="search"]',
    )
    if (!filter) throw new Error('filter was not rendered')
    filter.value = '127.0.0.1'
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    await element.updateComplete
    expect(rows(element)).toHaveLength(1)
    expect(rows(element)[0]?.textContent).toContain('lo')

    filter.value = ''
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    await element.updateComplete
    ;(
      element.shadowRoot?.querySelector(
        'button[aria-label="Sort by rx"]',
      ) as HTMLButtonElement
    ).click()
    await element.updateComplete

    expect(rows(element)[0]?.textContent).toContain('lo')
  })

  it('selects an interface and shows its details', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)

    rows(element)[0]?.click()
    await element.updateComplete

    expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain(
      'MTU',
    )
    expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain(
      '1500',
    )
    expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain(
      '2001:db8::10/64',
    )
  })

  it('brings an interface down after confirmation', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    const confirm = vi.fn(() => true)
    vi.stubGlobal('confirm', confirm)
    const listener = vi.fn()
    element.addEventListener(ANTH_NETWORK_STATE_EVENT, listener)

    button(element, 'Down').click()
    await flush(element)

    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('eth0'))
    expect(client.stateCalls).toEqual([{ name: 'eth0', up: false }])
    expect(listener).toHaveBeenCalledWith(
      expect.objectContaining({ detail: { name: 'eth0', up: false } }),
    )
  })

  it('does not change an interface when confirmation is cancelled', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    vi.stubGlobal('confirm', vi.fn(() => false))

    button(element, 'Down').click()
    await flush(element)

    expect(client.setState).not.toHaveBeenCalled()
  })

  it('hides controls when the daemon disables them', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([networkInterface()], {
      control_enabled: false,
    })
    const element = await mount(client)

    expect(findButton(element, 'Down')).toBeUndefined()
    expect(element.shadowRoot?.textContent).toContain(
      'Control is disabled by the daemon.',
    )
  })

  it('protects loopback from being brought down', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([
      networkInterface({
        name: 'lo',
        index: 1,
        state: 'unknown',
        loopback: true,
      }),
    ])
    const element = await mount(client)

    expect(element.shadowRoot?.textContent).toContain('protected')
    expect(findButton(element, 'Down')).toBeUndefined()
  })

  it('surfaces a state-change error', async () => {
    const client = new FakeNetworkClient()
    client.stateError = new Error('permission denied')
    const element = await mount(client)
    vi.stubGlobal('confirm', vi.fn(() => true))

    button(element, 'Down').click()
    await flush(element)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'permission denied',
    )

    client.emit(networkSnapshot())
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'permission denied',
    )
  })

  it('pauses, resumes and refreshes without duplicating the stream', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)

    button(element, 'Pause').click()
    await element.updateComplete
    expect(client.unsubscribes).toBe(1)
    expect(element.shadowRoot?.querySelector('.status')?.getAttribute('data-status')).toBe(
      'closed',
    )

    button(element, 'Resume').click()
    await flush(element)
    expect(client.watchCalls).toHaveLength(2)
    expect(client.list).toHaveBeenCalledTimes(2)

    button(element, 'Refresh').click()
    await flush(element)
    expect(client.list).toHaveBeenCalledTimes(3)
    expect(client.watchCalls).toHaveLength(2)
  })

  it('does not start automatically when autoStart is false', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client, { autoStart: false })

    expect(client.watch).not.toHaveBeenCalled()
    expect(client.list).not.toHaveBeenCalled()
    expect(button(element, 'Resume')).toBeTruthy()
  })

  it('reports a failed initial snapshot', async () => {
    const client = new FakeNetworkClient()
    client.listError = new Error('daemon unavailable')
    const element = await mount(client)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'daemon unavailable',
    )
  })
})
