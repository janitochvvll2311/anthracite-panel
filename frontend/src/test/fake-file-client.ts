import { vi } from 'vitest'
import type {
  FileClient,
  FileEntry,
  FileListing,
  FileReadResult,
  FileWriteResult,
} from '../lib/files/file-client'

export function fileEntry(overrides: Partial<FileEntry> = {}): FileEntry {
  return {
    name: 'file.txt',
    path: '/file.txt',
    kind: 'file',
    size: 10,
    modified_ms: 0,
    hidden: false,
    ...overrides,
  }
}

export function fileListing(overrides: Partial<FileListing> = {}): FileListing {
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

export function textResult(
  text: string,
  overrides: Partial<FileReadResult> = {},
): FileReadResult {
  return {
    blob: new Blob([text], { type: 'text/plain' }),
    size: new TextEncoder().encode(text).length,
    truncated: false,
    mime: 'text/plain',
    ...overrides,
  }
}

export class FakeFileClient implements FileClient {
  listings = new Map<string, FileListing>()
  readResults = new Map<string, FileReadResult>()
  listError?: Error
  readError?: Error
  mutationError?: Error
  mkdirCalls: Array<{ path: string; recursive: boolean }> = []
  renameCalls: Array<{ from: string; to: string }> = []
  deleteCalls: Array<{ path: string; recursive: boolean }> = []
  uploadCalls: Array<{ path: string; file: Blob; overwrite: boolean }> = []

  list = vi.fn(async (path: string): Promise<FileListing> => {
    if (this.listError) throw this.listError
    return this.listings.get(path) ?? fileListing({ path })
  })

  read = vi.fn(async (path: string): Promise<FileReadResult> => {
    if (this.readError) throw this.readError
    return this.readResults.get(path) ?? textResult('preview')
  })

  downloadUrl = vi.fn(
    (path: string) => `/api/files/download?path=${encodeURIComponent(path)}`,
  )

  mkdir = vi.fn(async (path: string, recursive = false): Promise<void> => {
    if (this.mutationError) throw this.mutationError
    this.mkdirCalls.push({ path, recursive })
  })

  rename = vi.fn(async (from: string, to: string): Promise<void> => {
    if (this.mutationError) throw this.mutationError
    this.renameCalls.push({ from, to })
  })

  delete = vi.fn(async (path: string, recursive = false): Promise<void> => {
    if (this.mutationError) throw this.mutationError
    this.deleteCalls.push({ path, recursive })
  })

  upload = vi.fn(
    async (
      path: string,
      file: Blob,
      overwrite = false,
    ): Promise<FileWriteResult> => {
      if (this.mutationError) throw this.mutationError
      this.uploadCalls.push({ path, file, overwrite })
      return { path, size: file.size }
    },
  )
}
