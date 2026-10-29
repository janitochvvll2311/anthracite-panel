import type {
  ArchiveCreated,
  ArchiveExtracted,
  ArchiveFormat,
  ArchiveListing,
  FileChunk,
  FileDeleted,
  FilePage,
  FileRestored,
  FileTrashed,
  FileWritten,
  FilesClient,
  TrashEmptied,
  TrashListing,
} from 'anth-wire'

import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpFilesClient implements FilesClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  async list(
    path: string,
    cursor?: string | null,
    limit?: number,
  ): Promise<FilePage> {
    const params = new URLSearchParams({ path })
    if (cursor) params.set('cursor', cursor)
    if (limit) params.set('limit', String(limit))
    return this.transport.json<FilePage>('GET', `/api/files?${params}`)
  }

  async read(
    path: string,
    offset = 0,
    maxBytes?: number,
  ): Promise<FileChunk> {
    const params = new URLSearchParams({ path, offset: String(offset) })
    if (maxBytes) params.set('max_bytes', String(maxBytes))
    return this.transport.json<FileChunk>('GET', `/api/files/read?${params}`)
  }

  download(path: string): Promise<Blob> {
    const params = new URLSearchParams({ path })
    return this.transport.blob(`/api/files/download?${params}`)
  }

  delete(path: string): Promise<FileDeleted> {
    const params = new URLSearchParams({ path })
    return this.transport.json<FileDeleted>('DELETE', `/api/files?${params}`)
  }

  write(
    path: string,
    content: string,
    create: boolean,
  ): Promise<FileWritten> {
    return this.transport.json<FileWritten>('POST', '/api/files/write', {
      path,
      content,
      create,
    })
  }

  upload(
    path: string,
    contentBase64: string,
    create: boolean,
  ): Promise<FileWritten> {
    return this.transport.json<FileWritten>('POST', '/api/files/upload', {
      path,
      content_base64: contentBase64,
      create,
    })
  }

  trash(path: string): Promise<FileTrashed> {
    return this.transport.json<FileTrashed>('POST', '/api/files/trash', {
      path,
    })
  }

  trashList(): Promise<TrashListing> {
    return this.transport.json<TrashListing>('GET', '/api/files/trash/list')
  }

  restore(id: string, overwrite = false): Promise<FileRestored> {
    return this.transport.json<FileRestored>('POST', '/api/files/trash/restore', {
      id,
      overwrite,
    })
  }

  emptyTrash(): Promise<TrashEmptied> {
    return this.transport.json<TrashEmptied>('POST', '/api/files/trash/empty')
  }

  archive(
    path: string,
    destination?: string,
    format?: ArchiveFormat,
  ): Promise<ArchiveCreated> {
    return this.transport.json<ArchiveCreated>('POST', '/api/files/archive', {
      path,
      destination,
      format,
    })
  }

  archiveList(path: string): Promise<ArchiveListing> {
    const params = new URLSearchParams({ path })
    return this.transport.json<ArchiveListing>(
      'GET',
      `/api/files/archive/list?${params}`,
    )
  }

  extractArchive(
    path: string,
    into?: string,
  ): Promise<ArchiveExtracted> {
    return this.transport.json<ArchiveExtracted>(
      'POST',
      '/api/files/archive/extract',
      { path, into },
    )
  }
}
