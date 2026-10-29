import { responseError } from './http'
import type { FilePage, FileChunk, FileWritten, ArchiveFormat, ArchiveListing, TrashListing, FilesClient } from 'anth-wire'

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

  async write(path: string, content: string, create: boolean): Promise<FileWritten> {
    const response = await this.fetchImpl('/api/files/write', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path, content, create }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FileWritten
  }

  async trash(path: string): Promise<{ path: string; id: string }> {
    const response = await this.fetchImpl('/api/files/trash', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as { path: string; id: string }
  }

  async trashList(): Promise<TrashListing> {
    const response = await this.fetchImpl('/api/files/trash/list')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as TrashListing
  }

  async restore(id: string, overwrite = false): Promise<void> {
    const response = await this.fetchImpl('/api/files/trash/restore', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id, overwrite }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async emptyTrash(): Promise<number> {
    const response = await this.fetchImpl('/api/files/trash/empty', { method: 'POST' })
    if (!response.ok) throw await responseError(response)
    return ((await response.json()) as { removed: number }).removed
  }

  async archive(path: string, destination?: string, format?: ArchiveFormat): Promise<string> {
    const response = await this.fetchImpl('/api/files/archive', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path, destination, format }),
    })
    if (!response.ok) throw await responseError(response)
    return ((await response.json()) as { path: string }).path
  }

  async archiveList(path: string): Promise<ArchiveListing> {
    const params = new URLSearchParams({ path })
    const response = await this.fetchImpl(`/api/files/archive/list?${params}`)
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as ArchiveListing
  }

  async extractArchive(
    path: string,
    into?: string,
  ): Promise<{ path: string; entries: string[] }> {
    const response = await this.fetchImpl('/api/files/archive/extract', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path, into }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as { path: string; entries: string[] }
  }
}
