import type {
  ArchiveCreated,
  ArchiveExtracted,
  FileDeleted,
  FileRestored,
  FileTrashed,
  TrashEmptied,
} from './actions'

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
  /** Streams any file as a blob; the same bytes `files download` prints. */
  download(path: string): Promise<Blob>
  delete(path: string): Promise<FileDeleted>
  write(path: string, content: string, create: boolean): Promise<FileWritten>
  upload(path: string, contentBase64: string, create: boolean): Promise<FileWritten>
  trash(path: string): Promise<FileTrashed>
  trashList(): Promise<TrashListing>
  restore(id: string, overwrite?: boolean): Promise<FileRestored>
  emptyTrash(): Promise<TrashEmptied>
  archive(path: string, destination?: string, format?: ArchiveFormat): Promise<ArchiveCreated>
  archiveList(path: string): Promise<ArchiveListing>
  extractArchive(path: string, into?: string): Promise<ArchiveExtracted>
}
