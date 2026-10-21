import { beforeEach, describe, expect, it, vi } from 'vitest'
import { FakeEventSource } from '../../test/fake-eventsource'
import { HttpProcessClient } from './process-client'
import type { ProcessHandlers, ProcessSnapshot } from './process-client'

function snapshot(overrides: Partial<ProcessSnapshot> = {}): ProcessSnapshot {
  return {
    processes: [
      {
        pid: 42,
        ppid: 1,
        uid: 1000,
        user: 'debian',
        name: 'bash',
        command: '/bin/bash',
        state: 'S',
        cpu_ticks: 10,
        rss_kib: 2048,
        threads: 1,
        start_ticks: 99,
      },
    ],
    clock_ticks: 100,
    timestamp_ms: 1000,
    control_enabled: false,
    spawn_enabled: false,
    spawn_allow: [],
    total: 1,
    truncated: false,
    ...overrides,
  }
}

function jsonResponse(
  body: unknown,
  init: { ok?: boolean; status?: number; statusText?: string } = {},
): Response {
  return {
    ok: init.ok ?? true,
    status: init.status ?? 200,
    statusText: init.statusText ?? 'OK',
    json: async () => body,
  } as Response
}

function handlers(): ProcessHandlers {
  return {
    onSnapshot: vi.fn(),
    onError: vi.fn(),
    onStatus: vi.fn(),
  }
}

describe('HttpProcessClient', () => {
  beforeEach(() => {
    FakeEventSource.reset()
  })

  it('lists processes from the configured base URL', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse(snapshot()))
    const client = new HttpProcessClient({
      baseUrl: '/custom/processes',
      fetchImpl,
    })

    const result = await client.list()

    expect(result.processes[0]?.pid).toBe(42)
    expect(fetchImpl).toHaveBeenCalledWith('/custom/processes', {
      headers: { accept: 'application/json' },
    })
  })

  it('turns an API error body into the thrown message', async () => {
    const client = new HttpProcessClient({
      fetchImpl: vi.fn(async () =>
        jsonResponse(
          { error: 'control is disabled' },
          { ok: false, status: 403, statusText: 'Forbidden' },
        ),
      ),
    })

    await expect(client.list()).rejects.toThrow('control is disabled')
  })

  it('keeps the status text when an error body is not JSON', async () => {
    const client = new HttpProcessClient({
      fetchImpl: vi.fn(async () => ({
        ok: false,
        status: 502,
        statusText: 'Bad Gateway',
        json: async () => {
          throw new Error('not json')
        },
      }) as unknown as Response),
    })

    await expect(client.list()).rejects.toThrow('502 Bad Gateway')
  })

  it('follows the SSE stream, clamps the interval and reports lifecycle', () => {
    const callbacks = handlers()
    const client = new HttpProcessClient({
      baseUrl: '/api/processes',
      EventSourceImpl: FakeEventSource as unknown as typeof EventSource,
    })

    const unsubscribe = client.watch(3.7, callbacks)
    const source = FakeEventSource.instances[0]
    expect(source?.url).toBe('/api/processes/watch?interval=10ms')
    expect(callbacks.onStatus).toHaveBeenCalledWith('connecting')

    source?.open()
    expect(callbacks.onStatus).toHaveBeenCalledWith('live')

    const next = snapshot({ total: 7 })
    source?.emit('processes', next)
    expect(callbacks.onSnapshot).toHaveBeenCalledWith(next)

    unsubscribe()
    expect(source?.closed).toBe(true)
    expect(callbacks.onStatus).toHaveBeenLastCalledWith('closed')
  })

  it('rounds the stream interval and reports parse and connection errors', () => {
    const callbacks = handlers()
    const client = new HttpProcessClient({
      EventSourceImpl: FakeEventSource as unknown as typeof EventSource,
    })

    client.watch(1500.4, callbacks)
    const source = FakeEventSource.instances[0]
    expect(source?.url).toBe('/api/processes/watch?interval=1500ms')

    source?.emit('processes', 'not json')
    expect(callbacks.onError).toHaveBeenCalledWith(expect.any(Error))

    source?.fail()
    expect(callbacks.onError).toHaveBeenLastCalledWith(
      expect.objectContaining({ message: 'process stream disconnected' }),
    )
    expect(callbacks.onStatus).toHaveBeenLastCalledWith('error')
  })

  it('posts a signal and uses the API error message on failure', async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(null, { status: 204, statusText: 'No Content' }))
      .mockResolvedValueOnce(
        jsonResponse(
          { error: 'pid 1 is protected' },
          { ok: false, status: 403, statusText: 'Forbidden' },
        ),
      )
    const client = new HttpProcessClient({ fetchImpl })

    await client.signal(42, 'term')
    expect(fetchImpl).toHaveBeenCalledWith('/api/processes/42/signal', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ signal: 'term' }),
    })

    await expect(client.signal(1, 'kill')).rejects.toThrow(
      'pid 1 is protected',
    )
  })

  it('posts a spawn request and returns the started process', async () => {
    const fetchImpl = vi.fn(async () =>
      jsonResponse({ pid: 4242, program: '/bin/echo', args: ['hi'] }),
    )
    const client = new HttpProcessClient({ fetchImpl })

    const started = await client.spawn({ program: '/bin/echo', args: ['hi'] })

    expect(started).toEqual({ pid: 4242, program: '/bin/echo', args: ['hi'] })
    expect(fetchImpl).toHaveBeenCalledWith('/api/processes/spawn', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ program: '/bin/echo', args: ['hi'] }),
    })
  })

  it('omits an absent cwd instead of sending null', async () => {
    // The daemon deserializes `cwd` as `#[serde(default)] Option<String>`; a
    // literal null would fail the body parse rather than mean "unset".
    const fetchImpl = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        jsonResponse({ pid: 1, program: '/bin/echo', args: [] }),
    )
    const client = new HttpProcessClient({ fetchImpl })

    await client.spawn({ program: '/bin/echo' })

    const body = JSON.parse(fetchImpl.mock.calls[0]![1]!.body as string)
    expect(body).toEqual({ program: '/bin/echo' })
    expect('cwd' in body).toBe(false)
    expect('args' in body).toBe(false)
  })

  it('uses the API error message when a spawn is refused', async () => {
    const client = new HttpProcessClient({
      fetchImpl: vi.fn(async () =>
        jsonResponse(
          { error: '/bin/sh is not in the allowlist' },
          { ok: false, status: 403, statusText: 'Forbidden' },
        ),
      ),
    })

    await expect(client.spawn({ program: '/bin/sh' })).rejects.toThrow(
      '/bin/sh is not in the allowlist',
    )
  })
})
