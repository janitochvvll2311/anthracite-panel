import { afterEach, expect, test, vi } from 'vitest'

import './processes'
import { HttpProcessesClient } from './processes-client'
import type {
  ProcessSnapshot,
  ProcessesClient,
  ProcessesHandlers,
} from './processes-client'

function snapshot(overrides: Partial<ProcessSnapshot> = {}): ProcessSnapshot {
  return {
    items: [
      {
        pid: 42,
        ppid: 1,
        uid: 0,
        user: 'root',
        name: 'sh',
        command: 'sh -c sleep 30',
        state: 'S',
        cpu_ticks: 100,
        rss_kib: 2048,
        threads: 1,
        start_ticks: 5,
      },
      {
        pid: 77,
        ppid: 42,
        uid: 1000,
        user: 'admin',
        name: 'sleep',
        command: 'sleep 30',
        state: 'R',
        cpu_ticks: 40,
        rss_kib: 1024,
        threads: 2,
        start_ticks: 9,
      },
    ],
    clock_ticks: 100,
    timestamp_ms: 1000,
    control_enabled: true,
    spawn_enabled: true,
    spawn_allow: ['/bin/sleep', '/bin/echo'],
    total: 2,
    ...overrides,
  }
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

type ProcessesElement = HTMLElement & {
  client: ProcessesClient
  updateComplete: Promise<boolean>
}

function fakeClient() {
  let handlers: ProcessesHandlers | undefined
  const close = vi.fn()
  const watch = vi.fn((_interval: number, next: ProcessesHandlers) => {
    handlers = next
    next.onStatus?.('live')
    return close
  })
  const signal = vi.fn().mockResolvedValue(undefined)
  const spawn = vi.fn().mockResolvedValue({
    pid: 99,
    program: '/bin/sleep',
    args: ['1'],
  })
  const list = vi.fn().mockResolvedValue(snapshot())
  const client: ProcessesClient = { list, watch, signal, spawn }
  return {
    client,
    watch,
    close,
    signal,
    spawn,
    push: (next: ProcessSnapshot) => handlers?.onSnapshot(next),
  }
}

function sizedRect(width: number): DOMRect {
  return {
    width,
    height: 600,
    top: 0,
    left: 0,
    bottom: 600,
    right: width,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect
}

async function mount(
  client: ProcessesClient,
  width?: number,
): Promise<ProcessesElement> {
  const element = document.createElement('anth-processes') as ProcessesElement
  if (width !== undefined) {
    element.getBoundingClientRect = () => sizedRect(width)
  }
  element.client = client
  document.body.append(element)
  await element.updateComplete
  await element.updateComplete
  return element
}

async function flush(): Promise<void> {
  for (let index = 0; index < 8; index += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
}

test('renders rows and computes CPU from two consecutive snapshots', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const first = element.shadowRoot?.textContent ?? ''
  expect(first).toContain('42')
  expect(first).toContain('sh -c sleep 30')
  expect(first).toContain('2 processes')

  feed.push(
    snapshot({
      timestamp_ms: 2000,
      items: snapshot().items.map((process) =>
        process.pid === 42 ? { ...process, cpu_ticks: 150 } : process,
      ),
    }),
  )
  await element.updateComplete

  const row = element.shadowRoot?.querySelector('.metrics-row[data-key="42:5"]')
  // 50 ticks over 1s on a 100 Hz clock is 50% of one core.
  expect(row?.textContent).toContain('50.0%')
})

test('filters rows by command, user or pid', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const search = element.shadowRoot?.querySelector<HTMLInputElement>(
    'input[type="search"]',
  )
  expect(search).not.toBeNull()
  search!.value = 'admin'
  search!.dispatchEvent(new Event('input'))
  await element.updateComplete

  const rows = element.shadowRoot?.querySelectorAll('.identity-row') ?? []
  expect(rows).toHaveLength(1)
  expect(rows[0].textContent).toContain('sleep 30')
  const text = element.shadowRoot?.textContent?.replace(/\s+/g, ' ') ?? ''
  expect(text).toContain('1 / 2 processes')
})

test('the view switch renders the tree and collapses branches', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const switches = element.shadowRoot?.querySelectorAll<HTMLButtonElement>(
    '.view-switch button',
  )
  switches?.[1].click()
  await element.updateComplete

  let rows = element.shadowRoot?.querySelectorAll('.identity-row') ?? []
  expect(rows).toHaveLength(2)
  expect(rows[0].getAttribute('aria-level')).toBe('1')
  expect(rows[1].getAttribute('aria-level')).toBe('2')

  element.shadowRoot?.querySelector<HTMLButtonElement>('.toggle')?.click()
  await element.updateComplete
  rows = element.shadowRoot?.querySelectorAll('.identity-row') ?? []
  expect(rows).toHaveLength(1)
})

test('selecting a row and sending a signal uses the row start_ticks', async () => {
  vi.stubGlobal('confirm', vi.fn(() => true))
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const rows = element.shadowRoot?.querySelectorAll<HTMLElement>('.identity-row') ?? []
  rows[0].click()
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.actions')).not.toBeNull()

  const signal = vi.fn().mockResolvedValue(undefined)
  feed.client.signal = signal
  const select = element.shadowRoot?.querySelector<HTMLSelectElement>(
    '.actions select',
  )
  expect(select?.value).toBe('term')
  element.shadowRoot
    ?.querySelector<HTMLButtonElement>('.actions .danger')
    ?.click()
  await flush()

  expect(signal).toHaveBeenCalledWith(42, 5, 'term', false)
  expect(element.shadowRoot?.textContent).toContain('Sent TERM to process 42')
})

test('the start form posts the typed command line and offers the allowlist', async () => {
  vi.stubGlobal('confirm', vi.fn(() => true))
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const open = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>(
    '.toolbar button',
  ) ?? [])].find((button) => button.textContent?.includes('Start process'))
  open?.click()
  await element.updateComplete

  const form = element.shadowRoot?.querySelector<HTMLFormElement>('.spawn')
  expect(form).not.toBeNull()
  const chips = form!.querySelectorAll<HTMLButtonElement>('.chip')
  expect([...chips].map((chip) => chip.textContent?.trim())).toEqual([
    '/bin/sleep',
    '/bin/echo',
  ])

  const command = form!.querySelector<HTMLInputElement>('input[name="command"]')
  command!.value = 'echo hello'
  command!.dispatchEvent(new Event('input'))
  form!.dispatchEvent(new Event('submit', { cancelable: true }))
  await flush()
  expect(feed.spawn).not.toHaveBeenCalled()
  expect(form!.textContent).toContain('absolute path')

  command!.value = '/bin/sleep 10 "a b"'
  command!.dispatchEvent(new Event('input'))
  form!.dispatchEvent(new Event('submit', { cancelable: true }))
  await flush()

  expect(feed.spawn).toHaveBeenCalledWith('/bin/sleep', ['10', 'a b'], undefined)
  expect(element.shadowRoot?.textContent).toContain('Started /bin/sleep (pid 99)')
})

test('an allowlist chip fills the program and keeps the arguments', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const open = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>(
    '.toolbar button',
  ) ?? [])].find((button) => button.textContent?.includes('Start process'))
  open?.click()
  await element.updateComplete

  const form = element.shadowRoot?.querySelector<HTMLFormElement>('.spawn')!
  const command = form.querySelector<HTMLInputElement>('input[name="command"]')!
  command.value = 'old --flag'
  command.dispatchEvent(new Event('input'))
  const chip = [...form.querySelectorAll<HTMLButtonElement>('.chip')].find(
    (candidate) => candidate.textContent?.trim() === '/bin/sleep',
  )
  chip?.click()
  await element.updateComplete
  expect(command.value).toBe('/bin/sleep --flag')
})

test('hides signals and start when their switches are off', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot({ control_enabled: false, spawn_enabled: false }))
  await element.updateComplete

  expect(element.shadowRoot?.querySelector('.toolbar button[aria-expanded]')).toBeNull()
  expect(element.shadowRoot?.querySelector('.actions .signal')).toBeNull()
  expect(element.shadowRoot?.textContent).toContain('Control is disabled by the daemon.')

  const row = element.shadowRoot?.querySelector<HTMLElement>('.identity-row')
  row?.click()
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.actions .signal')).toBeNull()
})

test('selection marks both panes and only the identity pane scrolls sideways', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const shadow = element.shadowRoot!
  expect(shadow.querySelector('.pane.identity')).not.toBeNull()
  expect(shadow.querySelector('.pane.metrics')).not.toBeNull()

  const row = shadow.querySelector<HTMLElement>('.identity-row')!
  row.click()
  await element.updateComplete

  expect(row.classList.contains('selected')).toBe(true)
  expect(row.getAttribute('aria-selected')).toBe('true')
  const key = row.getAttribute('data-key')
  const metrics = shadow.querySelector<HTMLElement>(
    `.metrics-row[data-key="${key}"]`,
  )
  expect(metrics?.classList.contains('selected')).toBe(true)
  expect(metrics?.getAttribute('aria-selected')).toBe('true')
})

test('the HTTP client sends the wire shape the daemon expects', async () => {
  const calls: Array<{ url: string; init?: RequestInit }> = []
  const fakeFetch = vi.fn((url: string, init?: RequestInit) => {
    calls.push({ url, init })
    if (url.startsWith('/api/processes/signal')) {
      return Promise.resolve(new Response(JSON.stringify({ pid: 42, signal: 'kill' })))
    }
    if (url.startsWith('/api/processes/spawn')) {
      return Promise.resolve(
        new Response(
          JSON.stringify({ pid: 99, program: '/bin/sleep', args: ['1'] }),
        ),
      )
    }
    return Promise.resolve(new Response(JSON.stringify(snapshot())))
  }) as unknown as typeof fetch
  const client = new HttpProcessesClient(fakeFetch)

  await expect(client.list(10, '20')).resolves.toMatchObject({ clock_ticks: 100 })
  expect(calls[0].url).toBe('/api/processes?limit=10&cursor=20')

  await client.signal(42, 5, 'kill', true)
  expect(JSON.parse(String(calls[1].init?.body))).toEqual({
    pid: 42,
    start_ticks: 5,
    signal: 'kill',
    group: true,
  })

  await client.spawn('/bin/sleep', ['1'], '/tmp')
  expect(JSON.parse(String(calls[2].init?.body))).toEqual({
    program: '/bin/sleep',
    args: ['1'],
    cwd: '/tmp',
  })
})

test('the HTTP client surfaces the daemon error text', async () => {
  const fakeFetch = (() =>
    Promise.resolve(
      new Response(
        JSON.stringify({
          error: { kind: 'forbidden', code: 'process_control_disabled', message: 'nope' },
        }),
        { status: 403 },
      ),
    )) as unknown as typeof fetch
  const client = new HttpProcessesClient(fakeFetch)
  await expect(client.signal(1, 1, 'term')).rejects.toThrow('nope')
})

test('mobile stacks the metrics under each row and the details follow the snapshot', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client, 700)
  feed.push(snapshot())
  await element.updateComplete

  const rows = element.shadowRoot?.querySelectorAll<HTMLElement>('.compact-row') ?? []
  expect(rows).toHaveLength(2)
  expect(element.shadowRoot?.querySelector('.pane.identity')).toBeNull()
  expect(rows[0].querySelector('.compact-metrics')?.textContent).toContain('root')
  expect(rows[0].querySelector('.compact-metrics')?.textContent).toContain('Sleeping')
  expect(rows[0].querySelector('.compact-metrics')?.textContent).toContain('2.0 MiB')

  rows[0].click()
  await element.updateComplete
  const details = element.shadowRoot?.querySelector('.selection-metrics')
  expect(details?.textContent).toContain('Sleeping')
  expect(details?.textContent).toContain('2.0 MiB')

  feed.push(
    snapshot({
      timestamp_ms: 2000,
      items: snapshot().items.map((process) =>
        process.pid === 42
          ? { ...process, rss_kib: 4096, state: 'R' }
          : process,
      ),
    }),
  )
  await element.updateComplete

  expect(element.shadowRoot?.querySelector('.selection-metrics')?.textContent).toContain(
    '4.0 MiB',
  )
  expect(element.shadowRoot?.querySelector('.selection-metrics')?.textContent).toContain(
    'Running',
  )
  const updated = element.shadowRoot?.querySelector(
    '.compact-row[data-key="42:5"] .compact-metrics',
  )
  expect(updated?.textContent).toContain('4.0 MiB')
})

test('the compact sort control orders the cards and flips direction', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client, 700)
  feed.push(snapshot())
  await element.updateComplete

  const select = element.shadowRoot?.querySelector<HTMLSelectElement>(
    '.filters select',
  )
  expect(select).not.toBeNull()
  select!.value = 'pid'
  select!.dispatchEvent(new Event('change'))
  await element.updateComplete
  const pids = () =>
    [...(element.shadowRoot?.querySelectorAll('.compact-row .pid') ?? [])].map(
      (cell) => cell.textContent?.trim(),
    )
  expect(pids()).toEqual(['42', '77'])

  element.shadowRoot?.querySelector<HTMLButtonElement>('.filters .direction')?.click()
  await element.updateComplete
  expect(pids()).toEqual(['77', '42'])
})

test('the compact tree keeps its gutter and collapses branches', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client, 700)
  feed.push(snapshot())
  await element.updateComplete

  const switches = element.shadowRoot?.querySelectorAll<HTMLButtonElement>(
    '.view-switch button',
  )
  switches?.[1].click()
  await element.updateComplete

  let rows = element.shadowRoot?.querySelectorAll<HTMLElement>('.compact-row') ?? []
  expect(rows).toHaveLength(2)
  expect(rows[0].querySelector('.compact-tree .tree-cell')).not.toBeNull()
  expect(rows[0].getAttribute('aria-level')).toBe('1')

  rows[0].querySelector<HTMLButtonElement>('.toggle')?.click()
  await element.updateComplete
  rows = element.shadowRoot?.querySelectorAll<HTMLElement>('.compact-row') ?? []
  expect(rows).toHaveLength(1)
})

test('the details bar mirrors the selected row on every snapshot', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  feed.push(snapshot())
  await element.updateComplete

  const rows = element.shadowRoot?.querySelectorAll<HTMLElement>('.identity-row') ?? []
  rows[1].click()
  await element.updateComplete

  const details = element.shadowRoot?.querySelector('.selection-metrics')
  expect(details?.textContent).toContain('Running')
  expect(details?.textContent).toContain('1.0 MiB')

  feed.push(
    snapshot({
      timestamp_ms: 2000,
      items: snapshot().items.map((process) =>
        process.pid === 77 ? { ...process, cpu_ticks: 140 } : process,
      ),
    }),
  )
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.selection-metrics')?.textContent).toContain(
    '100.0%',
  )
})

test('the layout switches when the identity side would be narrower than the metrics', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client, 700)
  feed.push(snapshot())
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.compact-list')).not.toBeNull()

  element.getBoundingClientRect = () => sizedRect(1200)
  feed.push(snapshot({ timestamp_ms: 2000 }))
  await element.updateComplete
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.compact-list')).toBeNull()
  expect(element.shadowRoot?.querySelector('.pane.identity')).not.toBeNull()

  element.getBoundingClientRect = () => sizedRect(700)
  feed.push(snapshot({ timestamp_ms: 3000 }))
  await element.updateComplete
  await element.updateComplete
  expect(element.shadowRoot?.querySelector('.compact-list')).not.toBeNull()
})
