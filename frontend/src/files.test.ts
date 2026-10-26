import { afterEach, expect, test, vi } from 'vitest'

import './files'
import type { FilePage, FilesClient } from './files-client'

const ROOT_PAGE: FilePage = {
  items: [
    { name: 'folder', kind: 'dir', size: 0, modified_ms: 0 },
    { name: 'note.txt', kind: 'file', size: 11, modified_ms: 0 },
    { name: 'other.txt', kind: 'file', size: 3, modified_ms: 0 },
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
    write: vi.fn(async (path: string) => ({ path, size: 0 })),
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

type WindowElement = HTMLElement & {
  shadowRoot: ShadowRoot
  updateComplete: Promise<boolean>
  minimized: boolean
}

type EditorElement = HTMLElement & {
  shadowRoot: ShadowRoot
  updateComplete: Promise<boolean>
}

/** Windows are global and render a couple of async steps behind the panel. */
async function settleWindows(): Promise<void> {
  for (let i = 0; i < 5; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
}

function lastWindow(): WindowElement | null {
  return document.querySelector<WindowElement>('anth-window')
}

function lastEditor(): EditorElement | null {
  return document.querySelector<EditorElement>('anth-file-editor')
}

async function openEditor(element: FilesElement, name: string): Promise<void> {
  const entry = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.entry') ?? [])].find(
    (button) => button.textContent?.includes(name),
  )
  entry?.click()
  await settle(element)
  const edit = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.preview .actions button') ?? [])].find(
    (button) => button.textContent?.includes('Edit'),
  )
  edit?.click()
  await settleWindows()
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

test('opens a file in a global editor window', async () => {
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
  await settleWindows()

  expect(client.read).toHaveBeenLastCalledWith('/note.txt')
  const window = lastWindow()
  const editor = lastEditor()
  expect(window).not.toBeNull()
  expect(editor).not.toBeNull()
  expect(element.shadowRoot?.querySelector('anth-window')).toBeNull()
  expect(editor?.shadowRoot.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe(
    'hello files',
  )

  window?.shadowRoot.querySelector<HTMLButtonElement>('.close')?.click()
  await settleWindows()
  expect(lastWindow()).toBeNull()
})

test('drags the editor window on desktop', async () => {
  const element = await mount(fakeClient())

  const create = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.tools button') ?? [])].find(
    (button) => button.textContent?.includes('New file'),
  )
  create?.click()
  await settle(element)
  await settleWindows()

  const window = lastWindow()
  const frame = window?.shadowRoot.querySelector<HTMLElement>('.frame')
  const header = window?.shadowRoot.querySelector<HTMLElement>('header')
  expect(window && frame && header).toBeTruthy()
  if (!window || !frame || !header) {
    return
  }

  // happy-dom has no layout, so give the drag code a measurable box.
  Object.defineProperty(frame, 'offsetWidth', { configurable: true, value: 400 })
  Object.defineProperty(frame, 'offsetHeight', { configurable: true, value: 300 })

  const down = new Event('pointerdown', { bubbles: true }) as PointerEvent
  Object.assign(down, { button: 0, clientX: 100, clientY: 100, pointerId: 7 })
  header.dispatchEvent(down)

  const move = new Event('pointermove', { bubbles: true }) as PointerEvent
  Object.assign(move, { clientX: 140, clientY: 120, pointerId: 7 })
  header.dispatchEvent(move)
  await window.updateComplete

  expect(frame.getAttribute('style')).toContain('--window-x: 40px')
  expect(frame.getAttribute('style')).toContain('--window-y: 20px')
})

test('keeps editor windows alive when the files panel unmounts', async () => {
  const element = await mount(fakeClient())

  const create = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.tools button') ?? [])].find(
    (button) => button.textContent?.includes('New file'),
  )
  create?.click()
  await settle(element)
  await settleWindows()

  const window = lastWindow()
  expect(window).not.toBeNull()

  element.remove()
  await settleWindows()

  expect(window?.isConnected).toBe(true)
  expect(lastEditor()).not.toBeNull()
})

test('restores an existing editor instead of opening a duplicate', async () => {
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
  await settleWindows()

  const window = lastWindow()
  window?.shadowRoot.querySelector<HTMLButtonElement>('.control-min')?.click()
  await window?.updateComplete
  expect(window?.minimized).toBe(true)

  edit?.click()
  await settleWindows()
  expect(document.querySelectorAll('anth-window')).toHaveLength(1)
  expect(lastWindow()?.minimized).toBe(false)
})

test('opens a second file after minimizing the first window', async () => {
  const element = await mount(fakeClient())
  await openEditor(element, 'note.txt')

  const first = lastWindow()
  first?.shadowRoot.querySelector<HTMLButtonElement>('.control-min')?.click()
  await first?.updateComplete
  expect(first?.minimized).toBe(true)
  // Minimizing must leave the explorer in place so another file can be opened.
  expect(element.isConnected).toBe(true)
  expect(element.shadowRoot?.querySelector('.layout')).not.toBeNull()

  await openEditor(element, 'other.txt')
  expect(document.querySelectorAll('anth-window')).toHaveLength(2)
  const minimized = [...document.querySelectorAll<WindowElement>('anth-window')].filter(
    (window) => window.minimized,
  )
  expect(minimized).toHaveLength(1)
})

test('navigates to a path typed in the breadcrumb', async () => {
  const client = fakeClient()
  const element = await mount(client)

  element.shadowRoot
    ?.querySelector<HTMLButtonElement>('.crumb[aria-current="page"]')
    ?.click()
  await settle(element)

  const input = element.shadowRoot?.querySelector<HTMLInputElement>('.path-edit input')
  expect(input?.value).toBe('/')
  if (input) {
    input.value = 'folder'
    input.dispatchEvent(new Event('input'))
  }
  element.shadowRoot
    ?.querySelector('form.path-edit')
    ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
  await settle(element)

  expect(client.list).toHaveBeenLastCalledWith('/folder')
  expect(element.shadowRoot?.querySelector('.crumbs')?.textContent?.replace(/\s+/g, '')).toBe(
    '/folder',
  )
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

test('opens a create window from the toolbar', async () => {
  const element = await mount(fakeClient())

  const create = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.tools button') ?? [])].find(
    (button) => button.textContent?.includes('New file'),
  )
  create?.click()
  await settle(element)
  await settleWindows()

  const editor = lastEditor()
  const name = editor?.shadowRoot.querySelector<HTMLInputElement>('input')
  expect(name).not.toBeNull()
  if (name) {
    name.value = 'draft.txt'
    name.dispatchEvent(new Event('input'))
  }
  await settleWindows()

  expect(editor?.shadowRoot.querySelector('.target')?.textContent).toBe('/draft.txt')
  expect(editor?.shadowRoot.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe('')
})

test('saves an edited file and reloads the preview', async () => {
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
  await settleWindows()

  const area = lastEditor()?.shadowRoot.querySelector<HTMLTextAreaElement>('textarea')
  expect(area).not.toBeNull()
  if (area) {
    area.value = 'updated'
    area.dispatchEvent(new Event('input'))
  }
  await settleWindows()
  lastEditor()?.shadowRoot.querySelector<HTMLButtonElement>('.save')?.click()
  await settleWindows()
  await settle(element)

  expect(client.write).toHaveBeenCalledWith('/note.txt', 'updated', false)
  expect(lastWindow()).toBeNull()
  expect(client.list).toHaveBeenCalledTimes(2)
  expect(element.shadowRoot?.querySelector('.preview')).not.toBeNull()
})

test('creates a file from the editor', async () => {
  const client = fakeClient()
  const element = await mount(client)

  const create = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.tools button') ?? [])].find(
    (button) => button.textContent?.includes('New file'),
  )
  create?.click()
  await settle(element)
  await settleWindows()

  const editor = lastEditor()
  const name = editor?.shadowRoot.querySelector<HTMLInputElement>('input')
  const area = editor?.shadowRoot.querySelector<HTMLTextAreaElement>('textarea')
  if (name && area) {
    name.value = 'draft.txt'
    name.dispatchEvent(new Event('input'))
    area.value = 'hello world'
    area.dispatchEvent(new Event('input'))
  }
  await settleWindows()
  editor?.shadowRoot.querySelector<HTMLButtonElement>('.save')?.click()
  await settleWindows()

  expect(client.write).toHaveBeenCalledWith('/draft.txt', 'hello world', true)
})
