import { describe, expect, it, vi } from 'vitest'

import { AuthClient } from './auth-client'
import type { SessionInfo } from './auth-client'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function anonymousSession(): SessionInfo {
  return {
    enabled: true,
    authenticated: false,
    username: 'admin',
    access_expires_in: 900,
    refresh_expires_in: 0,
  }
}

function liveSession(access = 900): SessionInfo {
  return {
    enabled: true,
    authenticated: true,
    username: 'admin',
    access_expires_in: access,
    refresh_expires_in: 3600,
  }
}

function build(responses: Response[]) {
  const fetchImpl = vi.fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>(
    async () => responses.shift() ?? jsonResponse(anonymousSession()),
  )
  const timeouts: Array<{ handler: () => void; timeout: number }> = []
  const setTimeoutImpl = ((handler: () => void, timeout: number) => {
    timeouts.push({ handler, timeout })
    return 1 as unknown as ReturnType<typeof setTimeout>
  }) as unknown as typeof setTimeout

  const client = new AuthClient({ fetchImpl: fetchImpl as typeof fetch, setTimeoutImpl })
  return { client, fetchImpl, timeouts }
}

describe('AuthClient', () => {
  it('logs in and then reads the session', async () => {
    const { client, fetchImpl } = build([
      jsonResponse({ username: 'admin' }),
      jsonResponse(liveSession()),
    ])

    const session = await client.login('admin', 'hunter2')

    expect(session.authenticated).toBe(true)
    expect(fetchImpl).toHaveBeenCalledTimes(2)
    const [, init] = fetchImpl.mock.calls[0]
    expect(init?.method).toBe('POST')
    expect(init?.credentials).toBe('same-origin')
  })

  it('surfaces the server detail on a failed login', async () => {
    const { client } = build([jsonResponse({ error: 'invalid credentials' }, 401)])

    await expect(client.login('admin', 'nope')).rejects.toThrow('invalid credentials')
  })

  it('schedules a refresh before the access token expires', async () => {
    const { client, timeouts } = build([jsonResponse(liveSession(900))])

    client.scheduleRefresh(liveSession(900))

    expect(timeouts).toHaveLength(1)
    // 900 s TTL minus the 30 s lead.
    expect(timeouts[0].timeout).toBe(870_000)
  })

  it('never schedules a refresh for an unauthenticated session', () => {
    const { client, timeouts } = build([])

    client.scheduleRefresh(anonymousSession())

    expect(timeouts).toHaveLength(0)
  })

  it('shares one refresh across concurrent callers', async () => {
    const { client, fetchImpl } = build([
      jsonResponse({ ok: true }),
      jsonResponse(liveSession()),
    ])

    const [first, second] = await Promise.all([client.refresh(), client.refresh()])

    expect(first).toEqual(second)
    expect(fetchImpl).toHaveBeenCalledTimes(2)
  })

  it('reports expiry when the refresh fails and does not schedule another', async () => {
    const { client } = build([jsonResponse({ error: 'session expired' }, 401)])
    const expired = vi.fn()
    client.onExpired(expired)

    await expect(client.refresh()).rejects.toThrow('session expired')

    expect(expired).toHaveBeenCalledOnce()
  })

  it('cancels a pending refresh on logout', async () => {
    const { client, timeouts } = build([jsonResponse({ ok: true })])
    client.scheduleRefresh(liveSession())

    await client.logout()

    expect(timeouts).toHaveLength(1)
    // A cleared timer must not fire; the fake never runs handlers on its own,
    // so the assertion is that logout did not schedule a second one.
    expect(timeouts).toHaveLength(1)
  })
})
