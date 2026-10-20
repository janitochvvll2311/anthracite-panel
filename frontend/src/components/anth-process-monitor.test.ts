import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-process-monitor'
import type { AnthProcessMonitor } from './anth-process-monitor'
import type { AnthProcessTable } from './anth-process-table'
import type {
  ProcessClient,
  ProcessHandlers,
  ProcessInfo,
  ProcessSignal,
  ProcessSnapshot,
} from '../lib/processes/process-client'

function processInfo(overrides: Partial<ProcessInfo> = {}): ProcessInfo {
  return {
    pid: 1,
    ppid: 0,
    uid: 1000,
    user: 'debian',
    name: 'init',
    command: '/sbin/init',
    state: 'S',
    cpu_ticks: 100,
    rss_kib: 1024,
    threads: 1,
    start_ticks: 10,
    ...overrides,
  }
}

function snapshot(
  processes: ProcessInfo[] = [processInfo()],
  overrides: Partial<ProcessSnapshot> = {},
): ProcessSnapshot {
  return {
    processes,
    clock_ticks: 100,
    timestamp_ms: 1000,
    control_enabled: false,
    total: processes.length,
    truncated: false,
    ...overrides,
  }
}

class FakeClient implements ProcessClient {
  queue: ProcessSnapshot[] = []
  current = snapshot()
  listCalls = 0
  watchCalls: number[] = []
  unsubscribes = 0
  handlers?: ProcessHandlers
  listError?: Error
  signalError?: Error
  signals: Array<{ pid: number; signal: ProcessSignal }> = []

  list = vi.fn(async () => {
    this.listCalls += 1
    if (this.listError) throw this.listError
    const next = this.queue.shift()
    if (next) this.current = next
    return this.current
  })

  watch = vi.fn((intervalMs: number, handlers: ProcessHandlers) => {
    this.watchCalls.push(intervalMs)
    this.handlers = handlers
    handlers.onStatus?.('connecting')
    return () => {
      this.unsubscribes += 1
      handlers.onStatus?.('closed')
    }
  })

  signal = vi.fn(async (pid: number, signal: ProcessSignal) => {
    this.signals.push({ pid, signal })
    if (this.signalError) throw this.signalError
  })

  emit(next: ProcessSnapshot): void {
    this.current = next
    this.handlers?.onSnapshot(next)
  }

  status(status: 'live' | 'error' | 'closed'): void {
    this.handlers?.onStatus?.(status)
  }

  fail(error: Error): void {
    this.handlers?.onError?.(error)
  }
}

interface UpdatableElement extends HTMLElement {
  updateComplete: Promise<boolean>
}

async function flush(element: AnthProcessMonitor): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete

    const tableElement = element.shadowRoot?.querySelector(
      'anth-process-table',
    ) as UpdatableElement | null
    await tableElement?.updateComplete
    const renderer = tableElement?.shadowRoot?.querySelector(
      'anth-process-flat-table, anth-process-tree',
    ) as UpdatableElement | null
    await renderer?.updateComplete
  }

  await element.updateComplete
}

async function mount(
  client = new FakeClient(),
  options: { autoStart?: boolean; refreshInterval?: number } = {},
): Promise<{ element: AnthProcessMonitor; client: FakeClient }> {
  const element = document.createElement(
    'anth-process-monitor',
  ) as AnthProcessMonitor
  element.client = client
  if (options.autoStart !== undefined) element.autoStart = options.autoStart
  if (options.refreshInterval !== undefined) {
    element.refreshInterval = options.refreshInterval
  }
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return { element, client }
}

function table(element: AnthProcessMonitor): AnthProcessTable {
  const child = element.shadowRoot?.querySelector(
    'anth-process-table',
  ) as AnthProcessTable | null
  if (!child) throw new Error('process table was not rendered')
  return child
}

function flatRow(element: AnthProcessMonitor): HTMLElement | null {
  return (
    table(element)
      .shadowRoot?.querySelector('anth-process-flat-table')
      ?.shadowRoot?.querySelector<HTMLElement>('tbody tr') ?? null
  )
}

function button(root: ShadowRoot, label: string): HTMLButtonElement {
  const match = [...root.querySelectorAll('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthProcessMonitor', () => {
  it('starts the stream and initial fetch, then exposes the live status', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot(
        [
          processInfo({ pid: 1, user: 'debian' }),
          processInfo({ pid: 2, user: 'root', command: '/usr/bin/postgres' }),
        ],
        { total: 2 },
      ),
    ]
    const { element } = await mount(client, { refreshInterval: 750 })

    expect(client.watchCalls).toEqual([750])
    expect(client.listCalls).toBe(1)
    expect(table(element).processes).toHaveLength(2)

    client.status('live')
    await element.updateComplete
    expect(
      element.shadowRoot?.querySelector('.status')?.getAttribute('data-status'),
    ).toBe('live')

    const root = element.shadowRoot
    expect(root?.querySelector('.count')?.textContent?.trim()).toBe(
      '2 / 2 processes',
    )
    expect(root?.querySelector('.status')?.textContent).toContain('live')
  })

  it('does not start automatically when autoStart is false', async () => {
    const { element, client } = await mount(new FakeClient(), {
      autoStart: false,
    })

    expect(client.watch).not.toHaveBeenCalled()
    expect(client.list).not.toHaveBeenCalled()
    expect(
      element.shadowRoot?.querySelector('.status')?.getAttribute('data-status'),
    ).toBe('idle')

    element.start()
    await flush(element)
    expect(client.watch).toHaveBeenCalledTimes(1)
    expect(client.listCalls).toBe(1)
  })

  it('computes CPU from consecutive snapshots and resets on start_ticks', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([processInfo({ pid: 7, cpu_ticks: 1000 })], {
        timestamp_ms: 1000,
      }),
    ]
    const { element } = await mount(client)
    expect(table(element).processes[0]?.cpu_percent).toBeNull()

    client.emit(
      snapshot([processInfo({ pid: 7, cpu_ticks: 1050 })], {
        timestamp_ms: 2000,
      }),
    )
    await flush(element)
    expect(table(element).processes[0]?.cpu_percent).toBeCloseTo(50)

    client.emit(
      snapshot(
        [processInfo({ pid: 7, cpu_ticks: 1100, start_ticks: 11 })],
        { timestamp_ms: 3000 },
      ),
    )
    await flush(element)
    expect(table(element).processes[0]?.cpu_percent).toBeNull()
  })

  it('ignores zero elapsed time and unknown clock ticks', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([processInfo({ pid: 7, cpu_ticks: 1000 })], {
        timestamp_ms: 1000,
      }),
    ]
    const { element } = await mount(client)

    client.emit(
      snapshot([processInfo({ pid: 7, cpu_ticks: 1050 })], {
        timestamp_ms: 1000,
      }),
    )
    await flush(element)
    expect(table(element).processes[0]?.cpu_percent).toBeNull()

    client.emit(
      snapshot([processInfo({ pid: 7, cpu_ticks: 1100 })], {
        timestamp_ms: 2000,
        clock_ticks: 0,
      }),
    )
    await flush(element)
    expect(table(element).processes[0]?.cpu_percent).toBeNull()
  })

  it('does not warn when only command lines were shortened', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([processInfo()], { total: 1, truncated: true }),
    ]
    const { element } = await mount(client)

    expect(table(element).warning).toBe('')
  })

  it('shows a row truncation warning when the daemon does not provide one', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([processInfo()], { total: 500, truncated: true }),
    ]
    const { element } = await mount(client)

    expect(table(element).warning).toBe('showing 1 of 500 processes')

    client.emit(
      snapshot([processInfo()], {
        total: 500,
        truncated: true,
        warning: 'commands were truncated',
      }),
    )
    await flush(element)
    expect(table(element).warning).toBe('commands were truncated')
  })

  it('filters by pid, user, name, command and state', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([
        processInfo({ pid: 1, user: 'debian', name: 'bash', command: '/bin/bash' }),
        processInfo({
          pid: 2,
          user: 'postgres',
          name: 'postgres',
          command: '/usr/bin/postgres -D /var/lib/postgresql',
          state: 'R',
        }),
      ]),
    ]
    const { element } = await mount(client)
    const input = element.shadowRoot?.querySelector<HTMLInputElement>(
      'input[type="search"]',
    )
    if (!input) throw new Error('filter input was not rendered')

    input.value = 'postgres'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await element.updateComplete
    expect(table(element).processes.map((process) => process.pid)).toEqual([2])
    expect(
      element.shadowRoot?.querySelector('.count')?.textContent?.trim(),
    ).toBe('1 / 2 processes')

    input.value = 'R'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await element.updateComplete
    expect(table(element).processes.map((process) => process.pid)).toEqual([2])

    input.value = 'missing'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await element.updateComplete
    expect(table(element).processes).toHaveLength(0)
  })

  it('switches between the flat and tree views', async () => {
    const { element } = await mount(new FakeClient())
    const root = element.shadowRoot
    expect(table(element).view).toBe('flat')

    button(root!, 'Tree').click()
    await flush(element)
    expect(table(element).view).toBe('tree')
    expect(button(root!, 'Tree').classList.contains('active')).toBe(true)
    expect(table(element).shadowRoot?.querySelector('anth-process-tree')).not.toBeNull()

    button(root!, 'Flat').click()
    await flush(element)
    expect(table(element).view).toBe('flat')
    expect(table(element).shadowRoot?.querySelector('anth-process-flat-table')).not.toBeNull()
  })

  it('disables signals when the daemon has control off', async () => {
    const { element } = await mount(new FakeClient())
    const root = element.shadowRoot

    expect(root?.querySelector('select[aria-label="Signal"]')).toBeNull()
    expect(root?.querySelector('.control-off')?.textContent).toContain(
      'Control is disabled',
    )
    expect(root?.textContent).toContain('Select a process to send a signal.')
  })

  it('selects a process, previews its full command and sends a signal', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([
        processInfo({
          pid: 42,
          user: 'debian',
          command: '/bin/bash -l --very-long-command',
        }),
      ], { control_enabled: true }),
    ]
    const { element } = await mount(client)
    const root = element.shadowRoot
    const selected = vi.fn()
    element.addEventListener('anth-process-signal', selected)
    vi.stubGlobal('confirm', vi.fn(() => true))

    flatRow(element)?.click()
    await flush(element)

    expect(root?.querySelector('.pid')?.textContent?.trim()).toBe('PID 42')
    expect(root?.querySelector('.selection .command')?.textContent?.trim()).toBe(
      '/bin/bash -l --very-long-command',
    )
    expect(root?.querySelector<HTMLButtonElement>('.signal button')?.disabled).toBe(
      false,
    )

    const select = root?.querySelector<HTMLSelectElement>('select')
    expect(select?.options).toHaveLength(7)
    if (!select) throw new Error('signal select was not rendered')
    select.value = 'kill'
    select.dispatchEvent(new Event('change', { bubbles: true }))
    await element.updateComplete

    root?.querySelector<HTMLButtonElement>('.signal button')?.click()
    await flush(element)

    expect(client.signals).toEqual([{ pid: 42, signal: 'kill' }])
    expect(selected).toHaveBeenCalledWith(
      expect.objectContaining({ detail: { pid: 42, signal: 'kill' } }),
    )
    expect(client.listCalls).toBe(2)
  })

  it('does not apply stream snapshots while a signal is in flight', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([processInfo({ pid: 42 })], { control_enabled: true }),
    ]
    const { element } = await mount(client)
    vi.stubGlobal('confirm', vi.fn(() => true))
    let finish!: () => void
    client.signal = vi.fn(
      (_pid: number, _signal: ProcessSignal) =>
        new Promise<void>((resolve) => {
          finish = resolve
        }),
    )

    flatRow(element)?.click()
    await flush(element)
    element.shadowRoot?.querySelector<HTMLButtonElement>('.signal button')?.click()
    await flush(element)

    client.emit(
      snapshot([processInfo({ pid: 99, command: '/usr/bin/postgres' })], {
        control_enabled: true,
      }),
    )
    await flush(element)

    expect(table(element).processes.map((process) => process.pid)).toEqual([42])
    expect(client.listCalls).toBe(1)

    finish()
    await flush(element)

    expect(table(element).processes.map((process) => process.pid)).toEqual([99])
    expect(client.listCalls).toBe(2)
  })

  it('does not signal when the confirmation is rejected', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([processInfo({ pid: 42 })], { control_enabled: true }),
    ]
    const { element } = await mount(client)
    vi.stubGlobal('confirm', vi.fn(() => false))

    flatRow(element)?.click()
    await flush(element)
    element.shadowRoot?.querySelector<HTMLButtonElement>('.signal button')?.click()
    await flush(element)

    expect(client.signal).not.toHaveBeenCalled()
    expect(client.listCalls).toBe(1)
  })

  it('reports a failed signal without dispatching the success event', async () => {
    const client = new FakeClient()
    client.queue = [
      snapshot([processInfo({ pid: 42 })], { control_enabled: true }),
    ]
    client.signalError = new Error('pid 1 is protected')
    const { element } = await mount(client)
    vi.stubGlobal('confirm', vi.fn(() => true))
    const selected = vi.fn()
    element.addEventListener('anth-process-signal', selected)

    flatRow(element)?.click()
    await flush(element)
    element.shadowRoot?.querySelector<HTMLButtonElement>('.signal button')?.click()
    await flush(element)

    expect(selected).not.toHaveBeenCalled()
    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('pid 1 is protected')
  })

  it('clears the selection when the process disappears', async () => {
    const client = new FakeClient()
    client.queue = [snapshot([processInfo({ pid: 1 })])]
    const { element } = await mount(client)

    flatRow(element)?.click()
    await flush(element)
    expect(element.shadowRoot?.querySelector('.pid')?.textContent).toContain(
      'PID 1',
    )

    client.emit(snapshot([processInfo({ pid: 2, command: '/usr/bin/postgres' })]))
    await flush(element)

    expect(element.shadowRoot?.querySelector('.pid')).toBeNull()
    expect(element.shadowRoot?.textContent).toContain(
      'Select a process to send a signal.',
    )
  })

  it('pauses, resumes and refreshes without duplicating the stream', async () => {
    const client = new FakeClient()
    const { element } = await mount(client)
    client.status('live')
    await element.updateComplete
    const root = element.shadowRoot

    button(root!, 'Pause').click()
    await element.updateComplete
    expect(client.unsubscribes).toBe(1)
    expect(
      root?.querySelector('.status')?.getAttribute('data-status'),
    ).toBe('closed')
    expect(button(root!, 'Resume')).toBeTruthy()

    button(root!, 'Resume').click()
    await flush(element)
    expect(client.watchCalls).toHaveLength(2)
    expect(client.listCalls).toBe(2)

    button(root!, 'Refresh').click()
    await flush(element)
    expect(client.listCalls).toBe(3)
    expect(client.watchCalls).toHaveLength(2)
  })

  it('surfaces list and stream errors', async () => {
    const client = new FakeClient()
    client.listError = new Error('daemon unavailable')
    const { element } = await mount(client)

    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('daemon unavailable')
    expect(
      element.shadowRoot?.querySelector('.status')?.getAttribute('data-status'),
    ).toBe('error')

    client.listError = undefined
    client.fail(new Error('process stream disconnected'))
    await element.updateComplete
    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('process stream disconnected')
  })
})
