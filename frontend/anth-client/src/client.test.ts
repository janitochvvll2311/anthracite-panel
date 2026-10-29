import { expect, test } from 'vitest'

import { AnthClient } from './client'
import { HttpTransport, responseError } from './http'

function fakeFetch(
  handler: (url: string, init?: RequestInit) => Response,
): typeof fetch {
  return ((input: RequestInfo | URL, init?: RequestInit) =>
    Promise.resolve(handler(String(input), init))) as typeof fetch
}

test('one transport carries the base address and the bearer token', async () => {
  const calls: Array<{ url: string; authorization: string | null }> = []
  const transport = new HttpTransport({
    baseUrl: 'https://panel.example/',
    token: 'secret',
    fetch: fakeFetch((url, init) => {
      calls.push({
        url,
        authorization: new Headers(init?.headers).get('authorization'),
      })
      return new Response(JSON.stringify({ protocol_version: 4 }), {
        headers: { 'content-type': 'application/json' },
      })
    }),
  })

  const client = new AnthClient(transport)

  await expect(client.hello()).resolves.toEqual({ protocol_version: 4 })
  expect(calls).toEqual([
    {
      url: 'https://panel.example/api/hello',
      authorization: 'Bearer secret',
    },
  ])
})

test('the flat client sends the same bodies as the Rust client', async () => {
  const calls: Array<{ url: string; method?: string; body?: unknown }> = []
  const client = new AnthClient({
    fetch: fakeFetch((url, init) => {
      calls.push({
        url,
        method: init?.method,
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      })
      return new Response(JSON.stringify({ message: 'ok' }), {
        headers: { 'content-type': 'application/json' },
      })
    }),
  })

  await client.networkSetState('eth0', false)
  await client.filesList('/etc', { limit: 5, cursor: 'abc' })
  await client.processSignal(7, 42, 'term')

  expect(calls).toEqual([
    {
      url: '/api/network/state',
      method: 'POST',
      body: { name: 'eth0', up: false },
    },
    {
      url: '/api/files?path=%2Fetc&cursor=abc&limit=5',
      method: 'GET',
      body: undefined,
    },
    {
      url: '/api/processes/signal',
      method: 'POST',
      body: { pid: 7, start_ticks: 42, signal: 'term', group: false },
    },
  ])
})

test('raw requests return the status instead of throwing', async () => {
  const client = new AnthClient({
    fetch: fakeFetch(() => new Response('no', { status: 503 })),
  })

  const response = await client.requestRaw('GET', '/api/healthz')
  expect(response.status).toBe(503)
  await expect(response.text()).resolves.toBe('no')
})

test('a parsed error carries kind, code and status', async () => {
  const error = await responseError(
    new Response(
      JSON.stringify({
        error: {
          kind: 'forbidden',
          code: 'process_control_disabled',
          message: 'process control is disabled',
        },
      }),
      { status: 403 },
    ),
  )

  expect(error.kind).toBe('forbidden')
  expect(error.code).toBe('process_control_disabled')
  expect(error.message).toBe('process control is disabled')
  expect(error.status).toBe(403)
})
