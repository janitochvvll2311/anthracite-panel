import { describe, expect, it, vi } from 'vitest'
import { HttpVolumeClient } from './volume-client'
import { volumeSnapshot } from '../../test/fake-volume-client'

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

describe('HttpVolumeClient', () => {
  it('fetches one snapshot from the configured base URL', async () => {
    const expected = volumeSnapshot()
    const fetchImpl = vi.fn(async () => jsonResponse(expected))
    const client = new HttpVolumeClient({
      baseUrl: '/custom/volumes',
      fetchImpl,
    })

    const result = await client.get()

    expect(result).toEqual(expected)
    expect(fetchImpl).toHaveBeenCalledWith('/custom/volumes', {
      headers: { accept: 'application/json' },
    })
  })

  it('turns a 403 body into the thrown message', async () => {
    const client = new HttpVolumeClient({
      fetchImpl: vi.fn(async () =>
        jsonResponse(
          { error: 'volume control is disabled' },
          { ok: false, status: 403, statusText: 'Forbidden' },
        ),
      ),
    })

    await expect(client.get()).rejects.toThrow('volume control is disabled')
  })

  it('posts a mount request with defaults filled in', async () => {
    const fetchImpl = vi.fn(async () =>
      jsonResponse({ message: 'mounted /dev/sdb1 on /mnt/data' }),
    )
    const client = new HttpVolumeClient({ fetchImpl })

    const message = await client.mount({
      device: '/dev/sdb1',
      target: '/mnt/data',
    })

    expect(message).toBe('mounted /dev/sdb1 on /mnt/data')
    expect(fetchImpl).toHaveBeenCalledWith('/api/volumes/mount', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        device: '/dev/sdb1',
        target: '/mnt/data',
        fs_type: undefined,
        read_only: false,
        allow_removable: false,
      }),
    })
  })

  it('forwards every mount option', async () => {
    const calls: Array<[RequestInfo | URL, RequestInit | undefined]> = []
    const fetchImpl = vi.fn(
      async (
        input: RequestInfo | URL,
        init?: RequestInit,
      ): Promise<Response> => {
        calls.push([input, init])
        return jsonResponse({ message: 'ok' })
      },
    )
    const client = new HttpVolumeClient({ fetchImpl })

    await client.mount({
      device: '/dev/sdb1',
      target: '/mnt/usb',
      fs_type: 'vfat',
      read_only: true,
      allow_removable: true,
    })

    const body = JSON.parse(calls[0]![1]!.body as string) as Record<
      string,
      unknown
    >
    expect(body).toEqual({
      device: '/dev/sdb1',
      target: '/mnt/usb',
      fs_type: 'vfat',
      read_only: true,
      allow_removable: true,
    })
  })

  it('posts an unmount request and surfaces a conflict', async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ message: 'unmounted /mnt/data' }))
      .mockResolvedValueOnce(
        jsonResponse(
          { error: '/mnt/data is busy' },
          { ok: false, status: 409, statusText: 'Conflict' },
        ),
      )
    const client = new HttpVolumeClient({ fetchImpl })

    expect(await client.unmount('/mnt/data')).toBe('unmounted /mnt/data')
    expect(fetchImpl).toHaveBeenLastCalledWith('/api/volumes/unmount', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ target: '/mnt/data' }),
    })
    await expect(client.unmount('/mnt/data')).rejects.toThrow('/mnt/data is busy')
  })
})
