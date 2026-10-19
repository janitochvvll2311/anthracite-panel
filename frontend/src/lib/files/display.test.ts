import { describe, expect, it } from 'vitest'
import type { FileEntry } from './file-client'
import {
  breadcrumbs,
  compareFileEntries,
  fileKindLabel,
  formatBytes,
  formatModified,
  joinPath,
  normalizePath,
  parentPath,
  sortFileEntries,
} from './display'

function entry(overrides: Partial<FileEntry> = {}): FileEntry {
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

describe('file display helpers', () => {
  it('normalizes virtual paths', () => {
    expect(normalizePath('')).toBe('/')
    expect(normalizePath('/')).toBe('/')
    expect(normalizePath('foo//bar/')).toBe('/foo/bar')
    expect(normalizePath('/foo/./bar')).toBe('/foo/bar')
    expect(joinPath('/', 'name')).toBe('/name')
    expect(joinPath('/a', 'b')).toBe('/a/b')
  })

  it('computes parents and breadcrumbs', () => {
    expect(parentPath('/')).toBeNull()
    expect(parentPath('/foo')).toBe('/')
    expect(parentPath('/foo/bar')).toBe('/foo')
    expect(breadcrumbs('/foo/bar')).toEqual([
      { name: '/', path: '/' },
      { name: 'foo', path: '/foo' },
      { name: 'bar', path: '/foo/bar' },
    ])
  })

  it('labels kinds and formats sizes', () => {
    expect(fileKindLabel('directory')).toBe('dir')
    expect(fileKindLabel('file')).toBe('file')
    expect(fileKindLabel('symlink')).toBe('link')
    expect(fileKindLabel('other')).toBe('other')
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(2048)).toBe('2.0 KiB')
    expect(formatBytes(3 * 1024 * 1024)).toBe('3.0 MiB')
    expect(formatBytes(2 * 1024 * 1024 * 1024)).toBe('2.0 GiB')
  })

  it('formats timestamps in UTC and null as a dash', () => {
    expect(formatModified(0)).toBe('1970-01-01 00:00 UTC')
    expect(formatModified(null)).toBe('—')
  })

  it('sorts directories first and honors the direction', () => {
    const a = entry({ name: 'a.txt', path: '/a.txt', size: 20 })
    const b = entry({ name: 'b.txt', path: '/b.txt', size: 10 })
    const dir = entry({ name: 'dir', path: '/dir', kind: 'directory', size: 0 })

    expect(sortFileEntries([a, b, dir], 'name', 'asc').map((e) => e.name)).toEqual([
      'dir',
      'a.txt',
      'b.txt',
    ])
    expect(sortFileEntries([a, b, dir], 'size', 'desc').map((e) => e.name)).toEqual([
      'dir',
      'a.txt',
      'b.txt',
    ])
    expect(compareFileEntries(a, b, 'name', 'desc')).toBeGreaterThan(0)
  })

  it('keeps unknown modification times last and breaks ties by name', () => {
    const unknown = entry({ name: 'z', modified_ms: null })
    const older = entry({ name: 'b', modified_ms: 1 })
    const newer = entry({ name: 'a', modified_ms: 2 })

    expect(
      sortFileEntries([unknown, newer, older], 'modified', 'asc').map((e) => e.name),
    ).toEqual(['b', 'a', 'z'])
    expect(
      sortFileEntries([unknown, newer, older], 'modified', 'desc').map((e) => e.name),
    ).toEqual(['a', 'b', 'z'])
  })
})
