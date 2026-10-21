import { describe, expect, it, vi } from 'vitest'

import { withRetry } from './http'

function response(status: number): Response {
  return new Response(status === 200 ? '{}' : '', { status })
}

describe('withRetry', () => {
  it('passes a success through untouched', async () => {
    const fetchImpl = vi.fn(async () => response(200))
    const auth = { refresh: vi.fn(async () => undefined) }

    const result = await withRetry(fetchImpl, auth)('/api/host-info')

    expect(result.status).toBe(200)
    expect(fetchImpl).toHaveBeenCalledTimes(1)
    expect(auth.refresh).not.toHaveBeenCalled()
  })

  it('refreshes once and retries after a 401', async () => {
    const fetchImpl = vi
      .fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>()
      .mockResolvedValueOnce(response(401))
      .mockResolvedValueOnce(response(200))
    const auth = { refresh: vi.fn(async () => undefined) }

    const result = await withRetry(fetchImpl, auth)('/api/host-info')

    expect(result.status).toBe(200)
    expect(auth.refresh).toHaveBeenCalledTimes(1)
    expect(fetchImpl).toHaveBeenCalledTimes(2)
  })

  it('shares one refresh between concurrent 401s', async () => {
    let release: (() => void) | undefined
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    const fetchImpl = vi
      .fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>()
      .mockResolvedValueOnce(response(401))
      .mockResolvedValueOnce(response(401))
      .mockResolvedValue(response(200))
    const auth = {
      refresh: vi.fn(async () => {
        await gate
      }),
    }
    const wrapped = withRetry(fetchImpl, auth)

    const first = wrapped('/api/a')
    const second = wrapped('/api/b')
    release?.()

    await Promise.all([first, second])
    expect(auth.refresh).toHaveBeenCalledTimes(1)
  })

  it('returns the original 401 when the refresh fails', async () => {
    const fetchImpl = vi.fn(async () => response(401))
    const auth = {
      refresh: vi.fn(async () => {
        throw new Error('expired')
      }),
    }

    const result = await withRetry(fetchImpl, auth)('/api/host-info')

    expect(result.status).toBe(401)
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('does not retry a refused login', async () => {
    const fetchImpl = vi.fn(async () => response(401))

    const result = await withRetry(fetchImpl, undefined)('/api/auth/login')

    expect(result.status).toBe(401)
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })
})
