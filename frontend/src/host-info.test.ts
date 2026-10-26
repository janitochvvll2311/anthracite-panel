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

test('renders the grouped host card from the stream', async () => {
  let handlers: HostInfoHandlers | undefined
  const client: HostInfoClient = {
    get: vi.fn().mockResolvedValue(sample),
    watch: vi.fn((_interval: number, next: HostInfoHandlers) => {
      handlers = next
      next.onStatus?.('live')
      return () => {}
    }),
  }

  const element = document.createElement('anth-host-info') as HTMLElement & {
    client: HostInfoClient
    updateComplete: Promise<boolean>
  }
  element.client = client
  document.body.append(element)
  await element.updateComplete
  handlers?.onSnapshot(sample)
  await element.updateComplete

  const text = element.shadowRoot?.textContent ?? ''
  expect(text).toContain('example')
  expect(text).toContain('Debian GNU/Linux')
  expect(text).toContain('Example CPU')
  expect(text).toContain('8 logical cores')
  expect(text).toContain('4.0 GiB / 8.0 GiB')
  expect(text).toContain('Not configured')
})

test('shows an error when the stream fails', async () => {
  let handlers: HostInfoHandlers | undefined
  const client: HostInfoClient = {
    get: vi.fn().mockResolvedValue(sample),
    watch: vi.fn((_interval: number, next: HostInfoHandlers) => {
      handlers = next
      return () => {}
    }),
  }

  const element = document.createElement('anth-host-info') as HTMLElement & {
    client: HostInfoClient
    updateComplete: Promise<boolean>
  }
  element.client = client
  document.body.append(element)
  await element.updateComplete
  handlers?.onError(new Error('stream down'))
  await element.updateComplete

  expect(element.shadowRoot?.textContent).toContain('stream down')
})
