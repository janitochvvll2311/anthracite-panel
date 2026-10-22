import { beforeEach, describe, expect, it, vi } from 'vitest'
import { FakeEventSource } from '../../test/fake-eventsource'
import { HttpHostInfoClient } from './host-info-client'
import type { HostInfoHandlers } from './host-info-client'
import { hostInfo } from '../../test/fake-host-info-client'

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

function handlers(): HostInfoHandlers {
  return {
    onSnapshot: vi.fn(),
    onError: vi.fn(),
    onStatus: vi.fn(),
  }
}

describe('HttpHostInfoClient', () => {
  beforeEach(() => {
    FakeEventSource.reset()
  })

  it('fetches one snapshot from the configured base URL', async () => {
    const expected = hostInfo()
    const fetchImpl = vi.fn(async () => jsonResponse(expected))
    const client = new HttpHostInfoClient({
      baseUrl: '/custom/host-info',
      fetchImpl,
    })

    const result = await client.get()

    expect(result).toEqual(expected)
    expect(fetchImpl).toHaveBeenCalledWith('/custom/host-info', {
      headers: { accept: 'application/json' },
    })
  })

  it('turns an API error body into the thrown message', async () => {
    const client = new HttpHostInfoClient({
      fetchImpl: vi.fn(async () =>
        jsonResponse(
          { error: 'daemon unavailable' },
          { ok: false, status: 503, statusText: 'Service Unavailable' },
        ),
      ),
    })

    await expect(client.get()).rejects.toThrow('daemon unavailable')
  })

  it('follows the SSE stream and reports lifecycle changes', () => {
    const callbacks = handlers()
    const client = new HttpHostInfoClient({
      baseUrl: '/api/host-info',
      EventSourceImpl: FakeEventSource as unknown as typeof EventSource,
    })

    const unsubscribe = client.watch(3.7, callbacks)
    const source = FakeEventSource.instances[0]

    expect(source?.url).toBe('/api/host-info/watch?interval=10ms')
    expect(callbacks.onStatus).toHaveBeenCalledWith('connecting')

    source?.open()
    expect(callbacks.onStatus).toHaveBeenCalledWith('live')

    const snapshot = hostInfo({ hostname: 'other-host' })
    source?.emit('host-info', snapshot)
    expect(callbacks.onSnapshot).toHaveBeenCalledWith(snapshot)

    unsubscribe()
    expect(source?.closed).toBe(true)
    expect(callbacks.onStatus).toHaveBeenLastCalledWith('closed')
  })

  it('rounds the interval and reports parse/connection errors', () => {
    const callbacks = handlers()
    const client = new HttpHostInfoClient({
      EventSourceImpl: FakeEventSource as unknown as typeof EventSource,
    })

    client.watch(1500.4, callbacks)
    const source = FakeEventSource.instances[0]
    expect(source?.url).toBe('/api/host-info/watch?interval=1500ms')

    source?.emit('host-info', 'not json')
    expect(callbacks.onError).toHaveBeenCalledWith(expect.any(Error))

    source?.fail()
    expect(callbacks.onError).toHaveBeenLastCalledWith(
      expect.objectContaining({ message: 'host info stream disconnected' }),
    )
    expect(callbacks.onStatus).toHaveBeenLastCalledWith('error')
  })
})
