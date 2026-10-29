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

export interface FileWritten {
  path: string
  size: number
}

export type ArchiveFormat = 'zip' | 'tar' | 'tar.gz' | 'tar.xz' | '7z'

export interface ArchiveEntry {
  name: string
  kind: 'file' | 'dir' | 'symlink' | 'other'
  size: number
  compressed?: number
}

export interface ArchiveListing {
  path: string
  format?: ArchiveFormat | null
  entries: ArchiveEntry[]
  total: number
  truncated: boolean
  warning?: string
}

export interface TrashEntry {
  id: string
  name: string
  original_path: string
  deleted_at?: string
  kind: 'file' | 'dir' | 'symlink' | 'other'
  size: number
  location: string
  stored_ms?: number
}

export interface TrashListing {
  entries: TrashEntry[]
  total: number
  truncated: boolean
  warning?: string
  enabled: boolean
  used_bytes: number
  max_bytes?: number | null
}

export interface FilesClient {
  list(path: string, cursor?: string | null, limit?: number): Promise<FilePage>
  read(path: string, offset?: number, maxBytes?: number): Promise<FileChunk>
  delete(path: string): Promise<void>
  write(path: string, content: string, create: boolean): Promise<FileWritten>
  trash(path: string): Promise<{ path: string; id: string }>
  trashList(): Promise<TrashListing>
  restore(id: string, overwrite?: boolean): Promise<void>
  emptyTrash(): Promise<number>
  archive(path: string, destination?: string, format?: ArchiveFormat): Promise<string>
  archiveList(path: string): Promise<ArchiveListing>
  extractArchive(path: string, into?: string): Promise<{ path: string; entries: string[] }>
}
