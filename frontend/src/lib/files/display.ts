import type { FileEntry, FileKind } from './file-client'

export type FileSortKey = 'name' | 'kind' | 'size' | 'modified'
export type FileSortDirection = 'asc' | 'desc'

export interface Breadcrumb {
  name: string
  path: string
}

/** Collapses empty and `.` components; the server owns the `..` policy. */
export function normalizePath(path: string): string {
  const parts = path.split('/').filter((part) => part !== '' && part !== '.')
  return parts.length === 0 ? '/' : `/${parts.join('/')}`
}

export function parentPath(path: string): string | null {
  const normalized = normalizePath(path)
  if (normalized === '/') return null
  const index = normalized.lastIndexOf('/')
  return index === 0 ? '/' : normalized.slice(0, index)
}

export function joinPath(parent: string, name: string): string {
  return normalizePath(`${normalizePath(parent)}/${name}`)
}

export function breadcrumbs(path: string): Breadcrumb[] {
  const normalized = normalizePath(path)
  const result: Breadcrumb[] = [{ name: '/', path: '/' }]
  if (normalized === '/') return result

  let current = ''
  for (const part of normalized.split('/').filter(Boolean)) {
    current += `/${part}`
    result.push({ name: part, path: current })
  }
  return result
}

export function fileKindLabel(kind: FileKind): string {
  switch (kind) {
    case 'directory':
      return 'dir'
    case 'file':
      return 'file'
    case 'symlink':
      return 'link'
    default:
      return 'other'
  }
}

export function formatBytes(bytes: number): string {
  const kib = 1024
  const mib = kib * 1024
  const gib = mib * 1024

  if (bytes >= gib) return `${(bytes / gib).toFixed(1)} GiB`
  if (bytes >= mib) return `${(bytes / mib).toFixed(1)} MiB`
  if (bytes >= kib) return `${(bytes / kib).toFixed(1)} KiB`
  return `${bytes} B`
}

export function formatModified(modified: number | null): string {
  if (modified === null) return '—'
  return `${new Date(modified).toISOString().slice(0, 16).replace('T', ' ')} UTC`
}

export function compareFileEntries(
  a: FileEntry,
  b: FileEntry,
  key: FileSortKey,
  direction: FileSortDirection,
): number {
  const aDirectory = a.kind === 'directory'
  const bDirectory = b.kind === 'directory'
  if (aDirectory !== bDirectory) return aDirectory ? -1 : 1

  let result = 0
  switch (key) {
    case 'name':
      result = compareText(a.name, b.name)
      break
    case 'kind':
      result = compareText(fileKindLabel(a.kind), fileKindLabel(b.kind))
      break
    case 'size':
      result = a.size - b.size
      break
    case 'modified': {
      if (a.modified_ms === null && b.modified_ms === null) {
        return compareText(a.name, b.name)
      }
      if (a.modified_ms === null) return 1
      if (b.modified_ms === null) return -1
      result = a.modified_ms - b.modified_ms
      break
    }
  }

  if (result === 0) result = compareText(a.name, b.name)
  return direction === 'asc' ? result : -result
}

export function sortFileEntries(
  entries: FileEntry[],
  key: FileSortKey,
  direction: FileSortDirection,
): FileEntry[] {
  return [...entries].sort((a, b) => compareFileEntries(a, b, key, direction))
}

function compareText(a: string, b: string): number {
  const lowerA = a.toLowerCase()
  const lowerB = b.toLowerCase()
  if (lowerA < lowerB) return -1
  if (lowerA > lowerB) return 1
  if (a < b) return -1
  if (a > b) return 1
  return 0
}
