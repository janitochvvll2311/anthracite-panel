import { describe, expect, it, vi } from 'vitest'
import { HttpAccountClient } from './account-client'
import { accountSnapshot } from '../../test/fake-account-client'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

function clientWith(handler: (url: string, init?: RequestInit) => Response) {
  const fetchImpl = vi.fn(async (url: string | URL | Request, init?: RequestInit) =>
    handler(String(url), init),
  )
  return { client: new HttpAccountClient({ fetchImpl: fetchImpl as typeof fetch }), fetchImpl }
}

describe('HttpAccountClient', () => {
  it('reads the snapshot from /api/accounts', async () => {
    const snapshot = accountSnapshot()
    const { client, fetchImpl } = clientWith(() => jsonResponse(snapshot))

    await expect(client.get()).resolves.toEqual(snapshot)
    expect(fetchImpl.mock.calls[0][0]).toBe('/api/accounts')
  })

  it('surfaces the daemon error detail instead of the status text', async () => {
    const { client } = clientWith(() => jsonResponse({ error: 'account control is disabled' }, 403))
    await expect(client.get()).rejects.toThrow('account control is disabled')
  })

  it('posts a create with every optional field', async () => {
    const { client, fetchImpl } = clientWith(() => jsonResponse({ message: 'created user bob' }))

    const message = await client.createUser({
      name: 'bob',
      uid: 1500,
      gid: 1500,
      gecos: 'Bob',
      home: '/home/bob',
      shell: '/bin/sh',
      system: true,
    })

    expect(message).toBe('created user bob')
    const [url, init] = fetchImpl.mock.calls[0]
    expect(url).toBe('/api/accounts/users')
    expect(init?.method).toBe('POST')
    expect(JSON.parse(String(init?.body))).toEqual({
      name: 'bob',
      uid: 1500,
      gid: 1500,
      gecos: 'Bob',
      home: '/home/bob',
      shell: '/bin/sh',
      system: true,
    })
  })

  it('escapes the name in the path', async () => {
    const { client, fetchImpl } = clientWith(() => jsonResponse({ message: 'ok' }))
    await client.modifyUser('a/b', { shell: '/bin/sh' })
    expect(fetchImpl.mock.calls[0][0]).toBe('/api/accounts/users/a%2Fb')
  })

  it('sends remove_home on delete', async () => {
    const { client, fetchImpl } = clientWith(() => jsonResponse({ message: 'ok' }))
    await client.deleteUser('bob', true)
    expect(fetchImpl.mock.calls[0][0]).toBe('/api/accounts/users/bob/delete')
    expect(JSON.parse(String(fetchImpl.mock.calls[0][1]?.body))).toEqual({ remove_home: true })
  })

  it('sends the password in the body, never in the URL', async () => {
    const { client, fetchImpl } = clientWith(() => jsonResponse({ message: 'ok' }))
    await client.setPassword('bob', 'correct-horse')

    const [url, init] = fetchImpl.mock.calls[0]
    expect(url).toBe('/api/accounts/users/bob/password')
    expect(url).not.toContain('correct-horse')
    expect(JSON.parse(String(init?.body))).toEqual({ password: 'correct-horse' })
  })

  it('falls back to a local message when the daemon sends none', async () => {
    const { client } = clientWith(() => jsonResponse({}))
    await expect(client.deleteGroup('devs')).resolves.toBe('deleted')
  })
})
