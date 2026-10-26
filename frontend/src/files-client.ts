import { responseError } from './http'

export interface FileEntry {
  name: string
  kind: 'file' | 'dir' | 'symlink' | 'other'
  size: number
  modified_ms: number
}

export interface FilePage {
  items: FileEntry[]
  next?: string | null
  total?: number | null
}

export interface FileChunk {
  path: string
  offset: number
  size: number
  eof: boolean
  text: string
}

export interface FilesClient {
  list(path: string, cursor?: string | null, limit?: number): Promise<FilePage>
  read(path: string, offset?: number, maxBytes?: number): Promise<FileChunk>
  delete(path: string): Promise<void>
}

export class HttpFilesClient implements FilesClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the other clients: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async list(path: string, cursor?: string | null, limit?: number): Promise<FilePage> {
    const params = new URLSearchParams({ path })
    if (cursor) params.set('cursor', cursor)
    if (limit) params.set('limit', String(limit))
    const response = await this.fetchImpl(`/api/files?${params}`)
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FilePage
  }

  async read(path: string, offset = 0, maxBytes?: number): Promise<FileChunk> {
    const params = new URLSearchParams({ path, offset: String(offset) })
    if (maxBytes) params.set('max_bytes', String(maxBytes))
    const response = await this.fetchImpl(`/api/files/read?${params}`)
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FileChunk
  }

  async delete(path: string): Promise<void> {
    const params = new URLSearchParams({ path })
    const response = await this.fetchImpl(`/api/files?${params}`, { method: 'DELETE' })
    if (!response.ok) throw await responseError(response)
  }
}
