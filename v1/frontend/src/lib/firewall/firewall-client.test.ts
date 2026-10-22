import { describe, expect, it, vi } from 'vitest'
import { HttpFirewallClient } from './firewall-client'
import { firewallSnapshot } from '../../test/fake-firewall-client'

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

describe('HttpFirewallClient', () => {
  it('fetches one snapshot from the configured base URL', async () => {
    const expected = firewallSnapshot()
    const fetchImpl = vi.fn(async () => jsonResponse(expected))
    const client = new HttpFirewallClient({
      baseUrl: '/custom/firewall',
      fetchImpl,
    })

    expect(await client.get()).toEqual(expected)
    expect(fetchImpl).toHaveBeenCalledWith('/custom/firewall', {
      headers: { accept: 'application/json' },
    })
  })

  it('posts a new rule and returns the daemon message', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse({ message: 'allowed tcp/80' }))
    const client = new HttpFirewallClient({ fetchImpl })

    const message = await client.addRule({
      action: 'allow',
      port: 80,
      protocol: 'tcp',
      comment: 'web',
    })

    expect(message).toBe('allowed tcp/80')
    expect(fetchImpl).toHaveBeenCalledWith('/api/firewall/rules', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        action: 'allow',
        port: 80,
        protocol: 'tcp',
        comment: 'web',
      }),
    })
  })

  it('posts delete and flush actions without a JSON body', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse({ message: 'done' }))
    const client = new HttpFirewallClient({ fetchImpl })

    await client.deleteRule(7)
    expect(fetchImpl).toHaveBeenLastCalledWith(
      '/api/firewall/rules/7/delete',
      expect.objectContaining({ method: 'POST', body: undefined }),
    )

    await client.flush()
    expect(fetchImpl).toHaveBeenLastCalledWith(
      '/api/firewall/flush',
      expect.objectContaining({ method: 'POST', body: undefined }),
    )
  })

  it('turns an API error body into the thrown message', async () => {
    const client = new HttpFirewallClient({
      fetchImpl: vi.fn(async () =>
        jsonResponse(
          { error: 'firewall control is disabled' },
          { ok: false, status: 403, statusText: 'Forbidden' },
        ),
      ),
    })

    await expect(client.flush()).rejects.toThrow('firewall control is disabled')
  })
})
