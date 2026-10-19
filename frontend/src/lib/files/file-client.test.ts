import { describe, expect, it, vi } from 'vitest'
import { HttpFileClient } from './file-client'
import type { FileListing } from './file-client'

function listing(overrides: Partial<FileListing> = {}): FileListing {
  return {
    path: '/',
    parent: null,
    entries: [],
    total: 0,
    truncated: false,
    write_enabled: false,
    ...overrides,
  }
}

function jsonResponse(body: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status: init.status ?? 200,
    statusText: init.statusText,
    headers: { 'content-type': 'application/json' },
  })
}

describe('HttpFileClient', () => {
  it('lists a directory with an encoded path', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse(listing({ path: '/docs' })))
    const client = new HttpFileClient({ fetchImpl })

    const result = await client.list('/docs with space')

    expect(result.path).toBe('/docs')
    expect(fetchImpl).toHaveBeenCalledWith(
      '/api/files?path=%2Fdocs+with+space',
      { headers: { accept: 'application/json' } },
    )
  })

  it('maps API errors to their message and falls back to the status', async () => {
    const client = new HttpFileClient({
      fetchImpl: vi
        .fn()
        .mockResolvedValueOnce(
          jsonResponse(
            { error: 'file writes are disabled' },
            { status: 403, statusText: 'Forbidden' },
          ),
        )
        .mockResolvedValueOnce(
          new Response('not json', { status: 502, statusText: 'Bad Gateway' }),
        ),
    })

    await expect(client.list('/')).rejects.toThrow('file writes are disabled')
    await expect(client.list('/')).rejects.toThrow('502 Bad Gateway')
  })

  it('reads a preview with its metadata headers', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(new Uint8Array([1, 2, 3]), {
          headers: {
            'content-type': 'image/png',
            'x-anth-size': '9',
            'x-anth-truncated': 'true',
          },
        }),
    )
    const client = new HttpFileClient({ fetchImpl })

    const result = await client.read('/image.png')

    expect(fetchImpl).toHaveBeenCalledWith('/api/files/content?path=%2Fimage.png', {
      headers: { accept: '*/*' },
    })
    expect(new Uint8Array(await result.blob.arrayBuffer())).toEqual(
      new Uint8Array([1, 2, 3]),
    )
    expect(result.mime).toBe('image/png')
    expect(result.size).toBe(9)
    expect(result.truncated).toBe(true)
  })

  it('builds a download URL', () => {
    const client = new HttpFileClient()
    expect(client.downloadUrl('/a b.txt')).toBe(
      '/api/files/download?path=%2Fa+b.txt',
    )
  })

  it('posts mkdir, rename and delete payloads', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse({}))
    const client = new HttpFileClient({ fetchImpl })

    await client.mkdir('/new', true)
    await client.rename('/a', '/b')
    await client.delete('/b', true)

    expect(fetchImpl).toHaveBeenNthCalledWith(1, '/api/files/mkdir', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ path: '/new', recursive: true }),
    })
    expect(fetchImpl).toHaveBeenNthCalledWith(2, '/api/files/rename', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ from: '/a', to: '/b' }),
    })
    expect(fetchImpl).toHaveBeenNthCalledWith(3, '/api/files/delete', {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ path: '/b', recursive: true }),
    })
  })

  it('uploads a blob as the raw body', async () => {
    const fetchImpl = vi.fn(async () =>
      jsonResponse({ path: '/uploaded.txt', size: 3 }),
    )
    const client = new HttpFileClient({ fetchImpl })
    const file = new Blob([new Uint8Array([1, 2, 3])], { type: 'text/plain' })

    const result = await client.upload('/uploaded.txt', file, true)

    expect(fetchImpl).toHaveBeenCalledWith(
      '/api/files/upload?path=%2Fuploaded.txt&overwrite=true',
      {
        method: 'POST',
        headers: {
          accept: 'application/json',
          'content-type': 'text/plain',
        },
        body: file,
      },
    )
    expect(result).toEqual({ path: '/uploaded.txt', size: 3 })
  })
})
