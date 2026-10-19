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
}

export interface FileReadResult {
  blob: Blob
  size: number
  truncated: boolean
  mime: string
}

export interface FileWriteResult {
  path: string
  size: number
}

export interface FileClient {
  list: (path: string) => Promise<FileListing>
  read: (path: string) => Promise<FileReadResult>
  downloadUrl: (path: string) => string
  mkdir: (path: string, recursive?: boolean) => Promise<void>
  rename: (from: string, to: string) => Promise<void>
  delete: (path: string, recursive?: boolean) => Promise<void>
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
