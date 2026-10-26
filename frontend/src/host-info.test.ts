import { afterEach, expect, test, vi } from 'vitest'

import './host-info'
import type { HostInfo, HostInfoClient, HostInfoHandlers } from './host-info-client'

const sample: HostInfo = {
  hostname: 'example',
  os: { name: 'Debian GNU/Linux', version: '13', kernel: '6.12.0', arch: 'x86_64' },
  uptime_seconds: 3 * 86_400 + 4 * 3_600 + 5 * 60 + 6,
  cpu_model: 'Example CPU',
  logical_cores: 8,
  load: { one: 0.12, five: 0.2, fifteen: 0.31 },
  memory: {
    total_kib: 8 * 1024 * 1024,
    available_kib: 4 * 1024 * 1024,
    swap_total_kib: 0,
    swap_free_kib: 0,
  },
  collected_at_ms: 1,
}

afterEach(() => {
  document.body.innerHTML = ''
})

type HostElement = HTMLElement & {
  client: HostInfoClient
  updateComplete: Promise<boolean>
}

function fakeClient() {
  let handlers: HostInfoHandlers | undefined
  const close = vi.fn()
  const watch = vi.fn((_interval: number, next: HostInfoHandlers) => {
    handlers = next
    next.onStatus?.('live')
    return close
  })
  const client: HostInfoClient = {
    get: vi.fn().mockResolvedValue(sample),
    watch,
  }
  return {
    client,
    watch,
    close,
    snapshot: () => handlers?.onSnapshot(sample),
  }
}

async function mount(client: HostInfoClient): Promise<HostElement> {
  const element = document.createElement('anth-host-info') as HostElement
  element.client = client
  document.body.append(element)
  await element.updateComplete
  return element
}

test('renders the summary, the resource cards and their bars', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.snapshot()
  await element.updateComplete

  const text = element.shadowRoot?.textContent ?? ''
  expect(text).toContain('example')
  expect(text).toContain('Debian GNU/Linux')
  expect(text).toContain('Example CPU')
  expect(text).toContain('8 logical cores')
  expect(text).toContain('Load average')
  expect(text).toContain('Not configured')

  // Three load bars, one memory bar; swap is not configured.
  expect(element.shadowRoot?.querySelectorAll('.bar')).toHaveLength(4)
})

test('pause closes the stream and resume opens a new one', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.snapshot()
  await element.updateComplete

  const button = element.shadowRoot?.querySelector('.toolbar button') as HTMLButtonElement
  expect(button.textContent).toContain('Pause')

  button.click()
  await element.updateComplete
  expect(feed.close).toHaveBeenCalledTimes(1)
  expect(button.textContent).toContain('Resume')
  expect(element.shadowRoot?.querySelector('.status')?.getAttribute('data-state')).toBe('paused')

  button.click()
  await element.updateComplete
  expect(feed.watch).toHaveBeenCalledTimes(2)
})

test('changing the interval restarts the stream on the new period', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  await element.updateComplete

  const select = element.shadowRoot?.querySelector('select') as HTMLSelectElement
  select.value = '5000'
  select.dispatchEvent(new Event('change'))
  await element.updateComplete

  expect(feed.watch).toHaveBeenLastCalledWith(5000, expect.anything())
})
