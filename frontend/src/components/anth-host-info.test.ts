import { afterEach, describe, expect, it } from 'vitest'
import './anth-host-info'
import type { AnthHostInfo } from './anth-host-info'
import {
  FakeHostInfoClient,
  hostInfo,
} from '../test/fake-host-info-client'

async function flush(element: AnthHostInfo): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  client: FakeHostInfoClient,
  options: { autoStart?: boolean; refreshInterval?: number } = {},
): Promise<AnthHostInfo> {
  const element = document.createElement('anth-host-info') as AnthHostInfo
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

function button(element: AnthHostInfo, label: string): HTMLButtonElement {
  const match = [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthHostInfo', () => {
  it('loads a snapshot and renders the host, CPU, load and memory', async () => {
    const client = new FakeHostInfoClient()
    const element = await mount(client)
    const root = element.shadowRoot

    expect(client.watchCalls).toEqual([2000])
    expect(client.get).toHaveBeenCalledTimes(1)
    expect(root?.querySelector('h2')?.textContent).toBe('node-1')
    expect(root?.textContent).toContain('Debian GNU/Linux')
    expect(root?.textContent).toContain('6.12.0-amd64')
    expect(root?.textContent).toContain('x86_64')
    expect(root?.textContent).toContain('1d 1h 1m 1s')
    expect(root?.textContent).toContain('Intel Xeon')
    expect(root?.textContent).toContain('4 logical cores')
    expect(root?.textContent).toContain('0.50')
    expect(root?.textContent).toContain('8.0 GiB')
    expect(root?.textContent).toContain('4.0 GiB')
    expect(root?.textContent).toContain('512.0 MiB')
    expect(root?.querySelectorAll('.card')).toHaveLength(4)
  })

  it('reports the live status and applies stream snapshots', async () => {
    const client = new FakeHostInfoClient()
    const element = await mount(client)
    const root = element.shadowRoot

    expect(root?.querySelector('.status-pill')?.getAttribute('data-status')).toBe(
      'connecting',
    )
    client.status('live')
    await element.updateComplete
    expect(root?.querySelector('.status-pill')?.getAttribute('data-status')).toBe(
      'live',
    )

    client.emit(hostInfo({ hostname: 'renamed-host' }))
    await element.updateComplete
    expect(root?.querySelector('h2')?.textContent).toBe('renamed-host')
  })

  it('does not start automatically when autoStart is false', async () => {
    const client = new FakeHostInfoClient()
    const element = await mount(client, { autoStart: false })

    expect(client.watch).not.toHaveBeenCalled()
    expect(client.get).not.toHaveBeenCalled()
    expect(button(element, 'Resume')).toBeTruthy()

    element.start()
    await flush(element)
    expect(client.watch).toHaveBeenCalledTimes(1)
    expect(client.get).toHaveBeenCalledTimes(1)
  })

  it('pauses, resumes and refreshes without duplicating the stream', async () => {
    const client = new FakeHostInfoClient()
    const element = await mount(client)
    client.status('live')
    await element.updateComplete

    button(element, 'Pause').click()
    await element.updateComplete
    expect(client.unsubscribes).toBe(1)
    expect(element.shadowRoot?.querySelector('.status-pill')?.getAttribute('data-status')).toBe(
      'closed',
    )

    button(element, 'Resume').click()
    await flush(element)
    expect(client.watchCalls).toHaveLength(2)
    expect(client.get).toHaveBeenCalledTimes(2)

    button(element, 'Refresh').click()
    await flush(element)
    expect(client.get).toHaveBeenCalledTimes(3)
    expect(client.watchCalls).toHaveLength(2)
  })

  it('restarts the stream when the interval changes', async () => {
    const client = new FakeHostInfoClient()
    const element = await mount(client)
    const select = element.shadowRoot?.querySelector<HTMLSelectElement>('select')
    if (!select) throw new Error('interval select was not rendered')

    select.value = '5000'
    select.dispatchEvent(new Event('change', { bubbles: true }))
    await flush(element)

    expect(element.refreshInterval).toBe(5000)
    expect(client.watchCalls).toEqual([2000, 5000])
  })

  it('surfaces fetch and stream errors', async () => {
    const client = new FakeHostInfoClient()
    client.getError = new Error('daemon unavailable')
    const element = await mount(client)
    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('daemon unavailable')
    expect(element.shadowRoot?.querySelector('.status-pill')?.getAttribute('data-status')).toBe(
      'error',
    )

    client.getError = undefined
    client.fail(new Error('host info stream disconnected'))
    await element.updateComplete
    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('host info stream disconnected')
  })

  it('renders the no-swap state', async () => {
    const client = new FakeHostInfoClient()
    client.current = hostInfo({
      memory: {
        total_kib: 1024,
        available_kib: 512,
        swap_total_kib: 0,
        swap_free_kib: 0,
      },
    })
    const element = await mount(client)

    expect(element.shadowRoot?.textContent).toContain('Not configured')
  })
})
