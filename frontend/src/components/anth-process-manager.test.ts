import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-process-manager'
import type { AnthProcessManager } from './anth-process-manager'
import type { AnthProcessTable } from './anth-process-table'
import type {
  ProcessClient,
  ProcessHandlers,
  ProcessInfo,
  ProcessSignal,
  ProcessSnapshot,
  SpawnRequest,
  SpawnedProcess,
} from '../lib/processes/process-client'
import { answerDialogs, dialogText, resetDialogs } from '../test/dialogs'
import { ANTH_TAB_SHOW_EVENT } from '../lib/ui/tab-visibility'

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
    spawn_enabled: false,
    spawn_allow: [],
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
  spawns: SpawnRequest[] = []
  spawnError?: Error
  spawned: SpawnedProcess = {
    pid: 4242,
    program: '/bin/echo',
    args: [],
  }

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

  spawn = vi.fn(async (request: SpawnRequest) => {
    this.spawns.push(request)
    if (this.spawnError) throw this.spawnError
    // Echo the request back the way the daemon does, so a test that checks the
    // reported pid and arguments is checking the component, not the fixture.
    return {
      ...this.spawned,
      program: request.program,
      args: request.args ?? [],
    }
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

async function flush(element: AnthProcessManager): Promise<void> {
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
): Promise<{ element: AnthProcessManager; client: FakeClient }> {
  const element = document.createElement(
    'anth-process-manager',
  ) as AnthProcessManager
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

function table(element: AnthProcessManager): AnthProcessTable {
  const child = element.shadowRoot?.querySelector(
    'anth-process-table',
  ) as AnthProcessTable | null
  if (!child) throw new Error('process table was not rendered')
  return child
}

function flatRow(element: AnthProcessManager): HTMLElement | null {
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
  resetDialogs()
  document.body.innerHTML = ''
})

describe('AnthProcessManager', () => {
  it('restarts the stream when a tab panel shows it again', async () => {
    const { element, client } = await mount()
    expect(client.watchCalls).toHaveLength(1)

    // What parking does: the panel removes the element from the document.
    element.remove()
    expect(client.unsubscribes).toBe(1)

    // Restoring changes no property, so `willUpdate` never runs; the show
    // event is the only signal a visible-again monitor gets.
    document.body.append(element)
    element.dispatchEvent(new CustomEvent(ANTH_TAB_SHOW_EVENT))
    await flush(element)

    expect(client.watchCalls).toHaveLength(2)
  })

  it('keeps a stream the operator paused while the tab was parked', async () => {
    const { element, client } = await mount()
    element.stop()
    expect(client.unsubscribes).toBe(1)

    element.remove()
    document.body.append(element)
    element.dispatchEvent(new CustomEvent(ANTH_TAB_SHOW_EVENT))
    await flush(element)

    expect(client.watchCalls).toHaveLength(1)
  })

  it('does not restart a paused monitor when it comes back', async () => {
    const { element, client } = await mount(new FakeClient(), {
      autoStart: false,
    })
    expect(client.watchCalls).toHaveLength(0)

    element.remove()
    document.body.append(element)
    element.dispatchEvent(new CustomEvent(ANTH_TAB_SHOW_EVENT))
    await flush(element)

    expect(client.watchCalls).toHaveLength(0)
  })

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
    expect(dialogText()).toContain('Send KILL?')
    await answerDialogs('confirm')
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
    await answerDialogs('confirm')
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

    flatRow(element)?.click()
    await flush(element)
    element.shadowRoot?.querySelector<HTMLButtonElement>('.signal button')?.click()
    await flush(element)
    await answerDialogs('cancel')
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
    const selected = vi.fn()
    element.addEventListener('anth-process-signal', selected)

    flatRow(element)?.click()
    await flush(element)
    element.shadowRoot?.querySelector<HTMLButtonElement>('.signal button')?.click()
    await flush(element)
    await answerDialogs('confirm')
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

  it('offers no start button when the daemon denies every spawn', async () => {
    // Enabling the switch without an allowlist opens nothing, so the form must
    // not be reachable either: an operator should not fill in a form whose
    // submission is certain to be refused.
    const client = new FakeClient()
    client.current = snapshot([processInfo()], {
      spawn_enabled: false,
      spawn_allow: [],
    })
    const { element } = await mount(client)

    const start = button(element.shadowRoot!, 'Start process')
    expect(start.disabled).toBe(true)
    expect(start.title).toContain('disabled')
  })

  it('renders a snapshot from a daemon that omits the allowlist', async () => {
    // A daemon predating `spawn_allow` sends no key at all. Reading `.length`
    // on `undefined` used to throw during render and take the whole panel down
    // with it, so absence has to mean "nothing allowed".
    const client = new FakeClient()
    const legacy = { ...snapshot(), spawn_enabled: true } as ProcessSnapshot
    delete (legacy as { spawn_allow?: string[] }).spawn_allow
    client.current = legacy

    const { element } = await mount(client)

    expect(element).toBeDefined()
    const start = button(element.shadowRoot!, 'Start process')
    expect(start.disabled).toBe(true)
    expect(start.title).toContain('spawn_allow')
  })

  it('explains an empty allowlist on the start button', async () => {
    const client = new FakeClient()
    client.current = snapshot([processInfo()], {
      spawn_enabled: true,
      spawn_allow: [],
    })
    const { element } = await mount(client)

    const start = button(element.shadowRoot!, 'Start process')
    expect(start.disabled).toBe(true)
    expect(start.title).toContain('spawn_allow')
  })

  it('spawns the typed command and reports the pid', async () => {
    const client = new FakeClient()
    client.current = snapshot([processInfo()], {
      spawn_enabled: true,
      spawn_allow: ['/bin/echo'],
    })
    const { element } = await mount(client)

    const events: Array<{ pid: number; program: string }> = []
    element.addEventListener('anth-process-spawned', (event) => {
      events.push((event as CustomEvent).detail)
    })

    button(element.shadowRoot!, 'Start process').click()
    await flush(element)

    const modal = element.shadowRoot?.querySelector('anth-modal')
    const command = modal?.querySelector<HTMLInputElement>('input[name="command"]')
    command!.value = '/bin/echo "hello there"'
    command!.dispatchEvent(new Event('input'))
    await element.updateComplete

    const form = modal?.querySelector('form')!
    form.dispatchEvent(new Event('submit'))
    await answerDialogs('confirm')
    await flush(element)

    expect(client.spawns).toHaveLength(1)
    expect(client.spawns[0]).toEqual({
      program: '/bin/echo',
      args: ['hello there'],
    })
    expect(events).toEqual([
      { pid: 4242, program: '/bin/echo', args: ['hello there'] },
    ])
  })

  it('keeps the form open and shows the daemon refusal', async () => {
    const client = new FakeClient()
    client.current = snapshot([processInfo()], {
      spawn_enabled: true,
      spawn_allow: ['/bin/echo'],
    })
    client.spawnError = new Error('`/bin/echo` is not in the allowlist')
    const { element } = await mount(client)

    button(element.shadowRoot!, 'Start process').click()
    await flush(element)

    const modal = element.shadowRoot?.querySelector('anth-modal')
    const command = modal?.querySelector<HTMLInputElement>('input[name="command"]')
    command!.value = '/bin/echo'
    command!.dispatchEvent(new Event('input'))
    await element.updateComplete

    modal?.querySelector('form')!.dispatchEvent(new Event('submit'))
    await answerDialogs('confirm')
    await flush(element)

    expect(modal?.open).toBe(true)
    expect(modal?.querySelector('[role="alert"]')?.textContent).toContain(
      'not in the allowlist',
    )
  })

  it('refuses a relative program before it reaches the daemon', async () => {
    const client = new FakeClient()
    client.current = snapshot([processInfo()], {
      spawn_enabled: true,
      spawn_allow: ['/bin/echo'],
    })
    const { element } = await mount(client)

    button(element.shadowRoot!, 'Start process').click()
    await flush(element)

    const modal = element.shadowRoot?.querySelector('anth-modal')
    const command = modal?.querySelector<HTMLInputElement>('input[name="command"]')
    command!.value = 'echo hi'
    command!.dispatchEvent(new Event('input'))
    await element.updateComplete

    modal?.querySelector('form')!.dispatchEvent(new Event('submit'))
    await flush(element)

    expect(client.spawns).toHaveLength(0)
    expect(modal?.querySelector('[role="alert"]')?.textContent).toContain(
      'absolute',
    )
  })

  it('fills the program from an allowlist chip and keeps the arguments', async () => {
    const client = new FakeClient()
    client.current = snapshot([processInfo()], {
      spawn_enabled: true,
      spawn_allow: ['/bin/echo', '/usr/bin/uptime'],
    })
    const { element } = await mount(client)

    button(element.shadowRoot!, 'Start process').click()
    await flush(element)

    const modal = element.shadowRoot?.querySelector('anth-modal')
    const command = modal?.querySelector<HTMLInputElement>('input[name="command"]')
    command!.value = '/bin/other --flag'
    command!.dispatchEvent(new Event('input'))
    await element.updateComplete

    const chip = [...modal!.querySelectorAll('button.chip')].find(
      (candidate) => candidate.textContent?.trim() === '/usr/bin/uptime',
    ) as HTMLButtonElement
    chip.click()
    await element.updateComplete

    expect(command!.value).toBe('/usr/bin/uptime --flag')
  })
})
