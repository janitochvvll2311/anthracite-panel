import { afterEach, expect, test, vi } from 'vitest'

import './files'
import type { FilePage, FilesClient } from './files-client'

const ROOT_PAGE: FilePage = {
  items: [
    { name: 'folder', kind: 'dir', size: 0, modified_ms: 0 },
    { name: 'note.txt', kind: 'file', size: 11, modified_ms: 0 },
  ],
  next: null,
}

const NESTED_PAGE: FilePage = {
  items: [{ name: 'nested.txt', kind: 'file', size: 3, modified_ms: 0 }],
  next: null,
}

function fakeClient(): FilesClient {
  return {
    list: vi.fn(async (path: string) => (path === '/folder' ? NESTED_PAGE : ROOT_PAGE)),
    read: vi.fn(async (path: string) => ({
      path,
      offset: 0,
      size: 11,
      eof: true,
      text: 'hello files',
    })),
    delete: vi.fn(async () => {}),
  }
}

type FilesElement = HTMLElement & {
  client: FilesClient
  updateComplete: Promise<boolean>
}

async function settle(element: FilesElement): Promise<void> {
  await element.updateComplete
  await new Promise((resolve) => setTimeout(resolve, 0))
  await element.updateComplete
}

async function mount(client: FilesClient): Promise<FilesElement> {
  const element = document.createElement('anth-files') as FilesElement
  element.client = client
  document.body.append(element)
  await settle(element)
  return element
}

afterEach(() => {
  document.body.innerHTML = ''
})

test('lists the root and navigates into a directory', async () => {
  const client = fakeClient()
  const element = await mount(client)
  expect(element.shadowRoot?.textContent).toContain('folder/')
  expect(element.shadowRoot?.textContent).toContain('note.txt')

  const folder = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.entry') ?? [])].find(
    (button) => button.textContent?.includes('folder'),
  )
  folder?.click()
  await settle(element)

  expect(client.list).toHaveBeenLastCalledWith('/folder')
  expect(element.shadowRoot?.textContent).toContain('nested.txt')
  // The root crumb is already `/`, so the first segment must not add another.
  expect(element.shadowRoot?.querySelector('.crumbs')?.textContent?.replace(/\s+/g, '')).toBe(
    '/folder',
  )
})

test('previews a file in place', async () => {
  const client = fakeClient()
  const element = await mount(client)

  const file = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.entry') ?? [])].find(
    (button) => button.textContent?.includes('note.txt'),
  )
  file?.click()
  await settle(element)

  expect(client.read).toHaveBeenCalledWith('/note.txt')
  expect(element.shadowRoot?.querySelector('pre')?.textContent).toBe('hello files')
  // The class drives the mobile swap: preview replaces the listing.
  expect(element.shadowRoot?.querySelector('.layout')?.classList.contains('preview-open')).toBe(true)
  const details = element.shadowRoot?.querySelector('.details')?.textContent?.replace(/\s+/g, ' ')
  expect(details).toContain('/note.txt')
  expect(details).toContain('File')
  expect(details).toContain('11 B')
})

test('shows a placeholder when a file cannot be previewed', async () => {
  const client = fakeClient()
  client.read = vi.fn().mockRejectedValue(new Error('not_text'))
  const element = await mount(client)

  const file = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.entry') ?? [])].find(
    (button) => button.textContent?.includes('note.txt'),
  )
  file?.click()
  await settle(element)

  expect(element.shadowRoot?.querySelector('.placeholder')?.textContent).toContain(
    'Preview not available for this file.',
  )
  expect(element.shadowRoot?.querySelector('.error')).toBeNull()
  expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain('/note.txt')
})

test('opens a file in the editor from the preview', async () => {
  const client = fakeClient()
  const element = await mount(client)

  const file = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.entry') ?? [])].find(
    (button) => button.textContent?.includes('note.txt'),
  )
  file?.click()
  await settle(element)

  const edit = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.preview .actions button') ?? [])].find(
    (button) => button.textContent?.includes('Edit'),
  )
  edit?.click()
  await settle(element)

  expect(client.read).toHaveBeenLastCalledWith('/note.txt')
  expect(element.shadowRoot?.querySelector('.editor-layer')).not.toBeNull()
  expect(element.shadowRoot?.querySelector('.files')?.classList.contains('editing')).toBe(true)
  expect(element.shadowRoot?.querySelector<HTMLTextAreaElement>('.editor textarea')?.value).toBe(
    'hello files',
  )

  element.shadowRoot?.querySelector<HTMLButtonElement>('.editor .close')?.click()
  await settle(element)
  expect(element.shadowRoot?.querySelector('.editor-layer')).toBeNull()
  expect(element.shadowRoot?.querySelector('.files')?.classList.contains('editing')).toBe(false)
})

test('deletes the previewed file after confirming', async () => {
  const client = fakeClient()
  const element = await mount(client)

  const file = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.entry') ?? [])].find(
    (button) => button.textContent?.includes('note.txt'),
  )
  file?.click()
  await settle(element)

  const remove = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.preview .actions button') ?? [])].find(
    (button) => button.textContent?.includes('Delete'),
  )
  remove?.click()
  await settle(element)
  expect(element.shadowRoot?.querySelector('.confirm')?.textContent).toContain('Delete this file?')

  const confirm = element.shadowRoot?.querySelector<HTMLButtonElement>('.confirm .danger')
  confirm?.click()
  await settle(element)

  expect(client.delete).toHaveBeenCalledWith('/note.txt')
  expect(client.list).toHaveBeenCalledTimes(2)
  expect(element.shadowRoot?.querySelector('.preview')).toBeNull()
})

test('creates a new file from the toolbar', async () => {
  const element = await mount(fakeClient())

  const create = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.tools button') ?? [])].find(
    (button) => button.textContent?.includes('New file'),
  )
  create?.click()
  await settle(element)

  const name = element.shadowRoot?.querySelector<HTMLInputElement>('.editor input')
  expect(name).not.toBeNull()
  if (name) {
    name.value = 'draft.txt'
    name.dispatchEvent(new Event('input'))
  }
  await settle(element)

  expect(element.shadowRoot?.querySelector('.editor .target')?.textContent).toBe('/draft.txt')
  expect(element.shadowRoot?.querySelector<HTMLTextAreaElement>('.editor textarea')?.value).toBe('')
})
