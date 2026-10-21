import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-file-manager'
import type { AnthFileManager } from './anth-file-manager'
import {
  FakeFileClient,
  fileEntry,
  fileListing,
  textResult,
} from '../test/fake-file-client'

import { answerDialogs, dialogText, resetDialogs } from '../test/dialogs'

async function flush(element: AnthFileManager): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  client: FakeFileClient,
  path = '/',
  options: { autoLoad?: boolean; showHidden?: boolean } = {},
): Promise<AnthFileManager> {
  const element = document.createElement(
    'anth-file-manager',
  ) as AnthFileManager
  element.client = client
  element.path = path
  if (options.autoLoad !== undefined) element.autoLoad = options.autoLoad
  if (options.showHidden !== undefined) element.showHidden = options.showHidden
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

function rows(element: AnthFileManager): HTMLElement[] {
  return [
    ...(element.shadowRoot?.querySelectorAll<HTMLElement>('tbody tr') ?? []),
  ]
}

function button(element: AnthFileManager, label: string): HTMLButtonElement {
  const match = [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
    (candidate) => candidate.textContent?.trim().startsWith(label),
  )
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

function selectFile(element: AnthFileManager, name: string): void {
  const row = rows(element).find((candidate) =>
    candidate.textContent?.includes(name),
  )
  if (!row) throw new Error(`row ${name} was not rendered`)
  row.click()
}

afterEach(() => {
  resetDialogs()
  document.body.innerHTML = ''
})

describe('AnthFileManager', () => {
  it('lists a directory with breadcrumbs and a read-only badge', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        entries: [
          fileEntry({ name: 'docs', path: '/docs', kind: 'directory', size: 0 }),
          fileEntry({ name: 'note.txt', path: '/note.txt', size: 2048 }),
          fileEntry({ name: '.secret', path: '/.secret', hidden: true }),
        ],
        total: 3,
      }),
    )

    const element = await mount(client)
    const root = element.shadowRoot

    expect(rows(element)).toHaveLength(2)
    expect(root?.querySelectorAll('.crumb')).toHaveLength(1)
    expect(root?.querySelector('.read-only')?.textContent?.trim()).toBe(
      'Read-only',
    )
    expect(root?.textContent).toContain('docs/')
    expect(root?.textContent).toContain('2.0 KiB')
    expect(button(element, 'Up').disabled).toBe(true)
    expect(
      [...(root?.querySelectorAll('button') ?? [])].some(
        (candidate) => candidate.textContent?.trim() === 'New folder',
      ),
    ).toBe(false)
  })

  it('navigates into a directory and back up', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        entries: [
          fileEntry({ name: 'docs', path: '/docs', kind: 'directory', size: 0 }),
        ],
      }),
    )
    client.listings.set(
      '/docs',
      fileListing({
        path: '/docs',
        parent: '/',
        entries: [fileEntry({ name: 'readme.md', path: '/docs/readme.md' })],
      }),
    )

    const element = await mount(client)
    rows(element)[0]?.dispatchEvent(
      new MouseEvent('dblclick', { bubbles: true }),
    )
    await flush(element)

    expect(element.path).toBe('/docs')
    expect(client.list).toHaveBeenLastCalledWith('/docs')
    expect(element.shadowRoot?.querySelectorAll('.crumb')).toHaveLength(2)
    expect(
      element.shadowRoot?.querySelector('.crumbs')?.textContent?.replace(/\s/g, ''),
    ).toBe('/docs')
    expect(element.shadowRoot?.textContent).toContain('readme.md')

    button(element, 'Up').click()
    await flush(element)
    expect(element.path).toBe('/')
  })

  it('toggles hidden entries without reloading', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        entries: [
          fileEntry({ name: 'visible.txt' }),
          fileEntry({ name: '.secret', path: '/.secret', hidden: true }),
        ],
      }),
    )
    const element = await mount(client)
    expect(rows(element)).toHaveLength(1)

    const toggle = element.shadowRoot?.querySelector<HTMLInputElement>(
      '.hidden-toggle input',
    )
    if (!toggle) throw new Error('hidden toggle was not rendered')
    toggle.checked = true
    toggle.dispatchEvent(new Event('change', { bubbles: true }))
    await element.updateComplete

    expect(rows(element)).toHaveLength(2)
    expect(client.list).toHaveBeenCalledTimes(1)
  })

  it('sorts a column and marks the active direction', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        entries: [
          fileEntry({ name: 'b.txt', path: '/b.txt', size: 20 }),
          fileEntry({ name: 'a.txt', path: '/a.txt', size: 10 }),
        ],
      }),
    )
    const element = await mount(client)

    const sizeHeader = [...(element.shadowRoot?.querySelectorAll('th') ?? [])].find(
      (header) => header.textContent?.includes('SIZE'),
    )
    sizeHeader?.querySelector('button')?.click()
    await element.updateComplete

    expect(sizeHeader?.getAttribute('aria-sort')).toBe('descending')
    expect(rows(element).map((row) => row.textContent)).toEqual([
      expect.stringContaining('b.txt'),
      expect.stringContaining('a.txt'),
    ])
  })

  it('previews text files and links the download', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        entries: [fileEntry({ name: 'note.txt', path: '/note.txt' })],
      }),
    )
    client.readResults.set('/note.txt', textResult('hello world'))

    const element = await mount(client)
    selectFile(element, 'note.txt')
    await flush(element)

    expect(client.read).toHaveBeenCalledWith('/note.txt')
    expect(
      element.shadowRoot?.querySelector('.text-preview')?.textContent,
    ).toBe('hello world')
    expect(
      element.shadowRoot?.querySelector<HTMLAnchorElement>('.download')?.href,
    ).toContain('/api/files/download?path=%2Fnote.txt')
  })

  it('offers the editor for a writable text file and saves with the loaded mtime', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        write_enabled: true,
        entries: [
          fileEntry({ name: 'note.txt', path: '/note.txt', modified_ms: 1234 }),
        ],
      }),
    )
    client.readResults.set('/note.txt', textResult('hello world'))

    const element = await mount(client)
    selectFile(element, 'note.txt')
    await flush(element)

    const toggle = element.shadowRoot?.querySelector<HTMLButtonElement>(
      '.edit-toggle',
    )
    expect(toggle?.textContent?.trim()).toBe('Edit')
    toggle?.click()
    await flush(element)

    const editor = element.shadowRoot?.querySelector('anth-file-editor')
    expect(editor).not.toBeNull()
    // The precondition is the mtime the listing reported, not the read time.
    expect(
      (editor as unknown as { save: unknown }).save,
    ).toBeTypeOf('function')
    await (
      editor as unknown as { save: (c: string) => Promise<unknown> }
    ).save('changed')
    expect(client.writeCalls).toEqual([
      { path: '/note.txt', contents: 'changed', expectedModifiedMs: 1234 },
    ])
  })

  it('hides the editor when writes are disabled', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        write_enabled: false,
        entries: [fileEntry({ name: 'note.txt', path: '/note.txt' })],
      }),
    )
    client.readResults.set('/note.txt', textResult('hello'))

    const element = await mount(client)
    selectFile(element, 'note.txt')
    await flush(element)

    expect(element.shadowRoot?.querySelector('.edit-toggle')).toBeNull()
  })

  it('hides the editor for a binary format', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        write_enabled: true,
        entries: [fileEntry({ name: 'archive.zip', path: '/archive.zip' })],
      }),
    )
    // A zip whose first bytes happen to be printable still must not be edited.
    client.readResults.set('/archive.zip', textResult('PK readable'))

    const element = await mount(client)
    selectFile(element, 'archive.zip')
    await flush(element)

    expect(element.shadowRoot?.querySelector('.edit-toggle')).toBeNull()
  })

  it('never renders SVG previews as active images', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        entries: [fileEntry({ name: 'icon.svg', path: '/icon.svg' })],
      }),
    )
    client.readResults.set('/icon.svg', {
      blob: new Blob(['<svg><script>alert(1)</script></svg>'], {
        type: 'text/plain',
      }),
      size: 39,
      truncated: false,
      mime: 'image/svg+xml',
    })

    const element = await mount(client)
    selectFile(element, 'icon.svg')
    await flush(element)

    expect(element.shadowRoot?.querySelector('.image-preview')).toBeNull()
    expect(element.shadowRoot?.querySelector('.text-preview')?.textContent).toContain(
      '<svg>',
    )
  })

  it('shows a binary message and a truncation warning', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        entries: [fileEntry({ name: 'blob.bin', path: '/blob.bin' })],
      }),
    )
    client.readResults.set('/blob.bin', {
      blob: new Blob([new Uint8Array([0, 1, 2])], {
        type: 'application/octet-stream',
      }),
      size: 100,
      truncated: true,
      mime: 'application/octet-stream',
    })

    const element = await mount(client)
    selectFile(element, 'blob.bin')
    await flush(element)

    expect(element.shadowRoot?.textContent).toContain('Binary file')
    expect(element.shadowRoot?.textContent).toContain('Preview truncated')
  })

  it('does not load automatically when autoLoad is false', async () => {
    const client = new FakeFileClient()
    const element = await mount(client, '/', { autoLoad: false })

    expect(client.list).not.toHaveBeenCalled()
    await element.load()
    expect(client.list).toHaveBeenCalledWith('/')
  })

  it('surfaces list and preview errors', async () => {
    const client = new FakeFileClient()
    client.listError = new Error('daemon unavailable')
    const element = await mount(client)
    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('daemon unavailable')

    client.listError = undefined
    await element.load()
    client.readError = new Error('permission denied')
    client.listings.set(
      '/',
      fileListing({ entries: [fileEntry({ name: 'secret' })] }),
    )
    await element.load()
    selectFile(element, 'secret')
    await flush(element)
    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('permission denied')
  })

  it('creates, renames and deletes when writes are enabled', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        write_enabled: true,
        entries: [fileEntry({ name: 'file.txt' })],
      }),
    )

    const element = await mount(client)
    button(element, 'New folder').click()
    await flush(element)
    await answerDialogs('confirm', 'renamed.txt')
    await flush(element)
    expect(client.mkdirCalls).toEqual([{ path: '/renamed.txt', recursive: false }])

    selectFile(element, 'file.txt')
    await flush(element)
    button(element, 'Rename').click()
    await flush(element)
    await answerDialogs('confirm', 'renamed.txt')
    await flush(element)
    expect(client.renameCalls).toEqual([
      { from: '/file.txt', to: '/renamed.txt' },
    ])

    selectFile(element, 'file.txt')
    await flush(element)
    button(element, 'Delete').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)
    expect(client.deleteCalls).toEqual([{ path: '/file.txt', recursive: false }])
  })

  it('does not reload the listing while a mutation is in flight', async () => {
    const client = new FakeFileClient()
    client.listings.set('/', fileListing({ write_enabled: true }))
    const element = await mount(client)
    let finish!: () => void
    client.mkdir = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve
        }),
    )

    button(element, 'New folder').click()
    await flush(element)
    await answerDialogs('confirm', 'new')
    await flush(element)

    expect(client.list).toHaveBeenCalledTimes(1)
    await element.load()
    expect(client.list).toHaveBeenCalledTimes(1)

    finish()
    await flush(element)

    expect(client.list).toHaveBeenCalledTimes(2)
  })

  it('uploads a selected file and asks before overwriting', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({
        write_enabled: true,
        entries: [fileEntry({ name: 'upload.txt' })],
      }),
    )

    const element = await mount(client)
    const input = element.shadowRoot?.querySelector<HTMLInputElement>(
      'input[type="file"]',
    )
    if (!input) throw new Error('upload input was not rendered')
    const file = new File(['abc'], 'upload.txt', { type: 'text/plain' })
    Object.defineProperty(input, 'files', {
      configurable: true,
      value: [file],
    })
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await flush(element)
    expect(dialogText()).toContain('Replace upload.txt?')
    await answerDialogs('confirm')
    await flush(element)

    expect(client.uploadCalls).toHaveLength(1)
    expect(client.uploadCalls[0]?.path).toBe('/upload.txt')
    expect(client.uploadCalls[0]?.overwrite).toBe(true)
  })

  it('reports mutation failures', async () => {
    const client = new FakeFileClient()
    client.listings.set(
      '/',
      fileListing({ write_enabled: true }),
    )
    client.mutationError = new Error('file writes are disabled')

    const element = await mount(client)
    button(element, 'New folder').click()
    await flush(element)
    await answerDialogs('confirm', 'new')
    await flush(element)

    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent?.trim(),
    ).toBe('file writes are disabled')
  })
})
