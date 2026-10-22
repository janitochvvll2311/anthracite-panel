/**
 * HTTP client for the filesystem endpoints.
 *
 * Components depend on this interface rather than on fetch directly, so tests
 * and alternative backends can inject their own implementation.
 */

import { responseError } from '../http'
import type { HttpClientOptions } from '../http'

export type FileKind = 'file' | 'directory' | 'symlink' | 'other'

export interface FileEntry {
  name: string
  path: string
  kind: FileKind
  size: number
  modified_ms: number | null
  hidden: boolean
  target?: string | null
}

export interface FileListing {
  path: string
  parent: string | null
  entries: FileEntry[]
  total: number
  truncated: boolean
  warning?: string
  write_enabled: boolean
  /** Delete moves entries to the trash instead of removing them. */
  trash_enabled?: boolean
}

export interface FileReadResult {
  blob: Blob
  size: number
  truncated: boolean
  mime: string
}

export interface TrashEntry {
  id: string
  name: string
  original_path: string
  deleted_at?: string | null
  kind: FileKind
  size: number
  location: string
}

export interface TrashListing {
  entries: TrashEntry[]
  total: number
  truncated: boolean
  warning?: string
  enabled: boolean
  used_bytes: number
}

export interface TrashResult {
  path: string
  id: string
}

export interface FileWriteResult {
  path: string
  size: number
}

export interface FileTextWriteResult {
  path: string
  size: number
  modified_ms: number
}

export interface FileClient {
  list: (path: string) => Promise<FileListing>
  read: (path: string) => Promise<FileReadResult>
  /**
   * Saves text. `expectedModifiedMs` is the mtime the editor loaded; the
   * daemon refuses the save when the file changed since, so two editors cannot
   * silently overwrite each other.
   */
  writeText: (
    path: string,
    contents: string,
    expectedModifiedMs?: number | null,
  ) => Promise<FileTextWriteResult>
  downloadUrl: (path: string) => string
  mkdir: (path: string, recursive?: boolean) => Promise<void>
  rename: (from: string, to: string) => Promise<void>
  delete: (path: string, recursive?: boolean) => Promise<void>
  /** Moves one entry to the trash and returns its identifier. */
  trash: (path: string) => Promise<TrashResult>
  listTrash: () => Promise<TrashListing>
  restore: (id: string, overwrite?: boolean) => Promise<void>
  emptyTrash: () => Promise<number>
  upload: (
    path: string,
    file: Blob,
    overwrite?: boolean,
  ) => Promise<FileWriteResult>
}

export interface HttpFileClientOptions extends HttpClientOptions {}

export class HttpFileClient implements FileClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch

  constructor(options: HttpFileClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api/files'
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
  }

  async list(path: string): Promise<FileListing> {
    const response = await this.fetchImpl(this.url('', path), {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FileListing
  }

  async read(path: string): Promise<FileReadResult> {
    const response = await this.fetchImpl(this.url('/content', path), {
      headers: { accept: '*/*' },
    })
    if (!response.ok) throw await responseError(response)

    return {
      blob: await response.blob(),
      size: Number(response.headers.get('x-anth-size') ?? 0),
      truncated: response.headers.get('x-anth-truncated') === 'true',
      mime:
        response.headers.get('content-type') ??
        response.headers.get('Content-Type') ??
        '',
    }
  }

  async writeText(
    path: string,
    contents: string,
    expectedModifiedMs?: number | null,
  ): Promise<FileTextWriteResult> {
    // The body is the raw text, not JSON: the daemon streams it in chunks, and
    // a JSON envelope would put the whole file in one socket line, which is
    // capped at 1 MiB.
    const precondition: Record<string, string> = {}
    if (expectedModifiedMs !== null && expectedModifiedMs !== undefined) {
      precondition.expected_modified_ms = String(expectedModifiedMs)
    }
    const response = await this.fetchImpl(
      this.url('/write', path, precondition),
      {
        method: 'POST',
        headers: {
          accept: 'application/json',
          'content-type': 'text/plain; charset=utf-8',
        },
        body: contents,
      },
    )
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FileTextWriteResult
  }

  downloadUrl(path: string): string {
    return this.url('/download', path)
  }

  async mkdir(path: string, recursive = false): Promise<void> {
    const response = await this.fetchImpl(`${this.baseUrl}/mkdir`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ path, recursive }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async rename(from: string, to: string): Promise<void> {
    const response = await this.fetchImpl(`${this.baseUrl}/rename`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ from, to }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async delete(path: string, recursive = false): Promise<void> {
    const response = await this.fetchImpl(`${this.baseUrl}/delete`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ path, recursive }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async trash(path: string): Promise<TrashResult> {
    const response = await this.fetchImpl(`${this.baseUrl}/trash`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ path }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as TrashResult
  }

  async listTrash(): Promise<TrashListing> {
    const response = await this.fetchImpl(`${this.baseUrl}/trash/list`, {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as TrashListing
  }

  async restore(id: string, overwrite = false): Promise<void> {
    const response = await this.fetchImpl(`${this.baseUrl}/trash/restore`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ id, overwrite }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async emptyTrash(): Promise<number> {
    const response = await this.fetchImpl(`${this.baseUrl}/trash/empty`, {
      method: 'POST',
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    const body = (await response.json()) as { removed: number }
    return body.removed
  }

  async upload(
    path: string,
    file: Blob,
    overwrite = false,
  ): Promise<FileWriteResult> {
    const response = await this.fetchImpl(
      this.url('/upload', path, { overwrite: String(overwrite) }),
      {
        method: 'POST',
        headers: {
          accept: 'application/json',
          'content-type': file.type || 'application/octet-stream',
        },
        body: file,
      },
    )
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FileWriteResult
  }

  private url(
    route: string,
    path: string,
    extra: Record<string, string> = {},
  ): string {
    const params = new URLSearchParams({ path, ...extra })
    return `${this.baseUrl}${route}?${params.toString()}`
  }
}
