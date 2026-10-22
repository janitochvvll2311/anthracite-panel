import { beforeEach, describe, expect, it, vi } from 'vitest'
import { FakeEventSource } from '../../test/fake-eventsource'
import { HttpNetworkClient } from './network-client'
import type { NetworkHandlers } from './network-client'
import { networkSnapshot } from '../../test/fake-network-client'

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

function handlers(): NetworkHandlers {
  return {
    onSnapshot: vi.fn(),
    onError: vi.fn(),
    onStatus: vi.fn(),
  }
}

describe('HttpNetworkClient', () => {
  beforeEach(() => {
    FakeEventSource.reset()
  })

  it('fetches one snapshot from the configured base URL', async () => {
    const expected = networkSnapshot()
    const fetchImpl = vi.fn(async () => jsonResponse(expected))
    const client = new HttpNetworkClient({
      baseUrl: '/custom/network',
      fetchImpl,
    })

    const result = await client.list()

    expect(result).toEqual(expected)
    expect(fetchImpl).toHaveBeenCalledWith('/custom/network', {
      headers: { accept: 'application/json' },
    })
  })

  it('turns an API error body into the thrown message', async () => {
    const client = new HttpNetworkClient({
      fetchImpl: vi.fn(async () =>
        jsonResponse(
          { error: 'network control is disabled' },
          { ok: false, status: 403, statusText: 'Forbidden' },
        ),
      ),
    })

    await expect(client.list()).rejects.toThrow('network control is disabled')
  })

  it('follows the SSE stream and reports lifecycle changes', () => {
    const callbacks = handlers()
    const client = new HttpNetworkClient({
      baseUrl: '/api/network',
      EventSourceImpl: FakeEventSource as unknown as typeof EventSource,
    })

    const unsubscribe = client.watch(3.7, callbacks)
    const source = FakeEventSource.instances[0]

    expect(source?.url).toBe('/api/network/watch?interval=10ms')
    expect(callbacks.onStatus).toHaveBeenCalledWith('connecting')

    source?.open()
    expect(callbacks.onStatus).toHaveBeenCalledWith('live')

    const snapshot = networkSnapshot()
    source?.emit('network', snapshot)
    expect(callbacks.onSnapshot).toHaveBeenCalledWith(snapshot)

    unsubscribe()
    expect(source?.closed).toBe(true)
    expect(callbacks.onStatus).toHaveBeenLastCalledWith('closed')
  })

  it('reports parse and connection errors', () => {
    const callbacks = handlers()
    const client = new HttpNetworkClient({
      EventSourceImpl: FakeEventSource as unknown as typeof EventSource,
    })

    client.watch(1500.4, callbacks)
    const source = FakeEventSource.instances[0]
    expect(source?.url).toBe('/api/network/watch?interval=1500ms')

    source?.emit('network', 'not json')
    expect(callbacks.onError).toHaveBeenCalledWith(expect.any(Error))

    source?.fail()
    expect(callbacks.onError).toHaveBeenLastCalledWith(
      expect.objectContaining({ message: 'network stream disconnected' }),
    )
    expect(callbacks.onStatus).toHaveBeenLastCalledWith('error')
  })

  it('posts a state change with an encoded interface name', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse({ name: 'eth0', up: false }))
    const client = new HttpNetworkClient({
      baseUrl: '/api/network',
      fetchImpl,
    })

    await client.setState('eth0 1', false)

    expect(fetchImpl).toHaveBeenCalledWith('/api/network/eth0%201/state', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ up: false }),
    })
  })

  it('surfaces a state-change error', async () => {
    const client = new HttpNetworkClient({
      fetchImpl: vi.fn(async () =>
        jsonResponse(
          { error: 'refusing to bring the loopback interface down' },
          { ok: false, status: 403, statusText: 'Forbidden' },
        ),
      ),
    })

    await expect(client.setState('lo', false)).rejects.toThrow('loopback')
  })
})

describe('management requests', () => {
  it('creates through POST /api/network', async () => {
    const fetchMock = vi.fn(
      async () => new Response(null, { status: 200 }),
    ) as unknown as typeof fetch
    const client = new HttpNetworkClient({ fetchImpl: fetchMock })

    await client.create({ name: 'br0', kind: 'bridge', mtu: 1500, up: true })

    const [url, init] = (fetchMock as unknown as ReturnType<typeof vi.fn>).mock
      .calls[0]
    expect(url).toBe('/api/network')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body)).toEqual({
      name: 'br0',
      kind: 'bridge',
      mtu: 1500,
      up: true,
    })
  })

  it('edits with PATCH and only the fields the caller passed', async () => {
    // An MTU edit must not carry `up: false`, which would take the link down.
    const fetchMock = vi.fn(
      async () => new Response(null, { status: 200 }),
    ) as unknown as typeof fetch
    const client = new HttpNetworkClient({ fetchImpl: fetchMock })

    await client.edit('eth0', { mtu: 9000 })

    const [url, init] = (fetchMock as unknown as ReturnType<typeof vi.fn>).mock
      .calls[0]
    expect(url).toBe('/api/network/eth0')
    expect(init.method).toBe('PATCH')
    expect(JSON.parse(init.body)).toEqual({ mtu: 9000 })
  })

  it('renames and deletes through their own sub-routes', async () => {
    const fetchMock = vi.fn(
      async () => new Response(null, { status: 200 }),
    ) as unknown as typeof fetch
    const client = new HttpNetworkClient({ fetchImpl: fetchMock })

    await client.rename('d0', 'd1')
    await client.remove('eth0')

    const calls = (fetchMock as unknown as ReturnType<typeof vi.fn>).mock.calls
    expect(calls[0][0]).toBe('/api/network/d0/rename')
    expect(JSON.parse(calls[0][1].body)).toEqual({ new_name: 'd1' })
    expect(calls[1][0]).toBe('/api/network/eth0/delete')
  })

  it('surfaces the daemon error body', async () => {
    const fetchMock = vi.fn(
      async () =>
        new Response(JSON.stringify({ error: 'refusing to delete `eth0`' }), {
          status: 403,
          headers: { 'content-type': 'application/json' },
        }),
    ) as unknown as typeof fetch
    const client = new HttpNetworkClient({ fetchImpl: fetchMock })

    await expect(client.remove('eth0')).rejects.toThrow('refusing to delete')
  })
})
