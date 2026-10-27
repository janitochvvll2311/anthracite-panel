import { afterEach, expect, test, vi } from 'vitest'

import './files'
import type { FilePage, FilesClient } from './files-client'

const ROOT_PAGE: FilePage = {
  items: [
    { name: 'folder', kind: 'dir', size: 0, modified_ms: 0 },
    { name: 'note.txt', kind: 'file', size: 11, modified_ms: 0 },
    { name: 'other.txt', kind: 'file', size: 3, modified_ms: 0 },
    { name: 'photo.png', kind: 'file', size: 4, modified_ms: 0 },
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
    upload: vi.fn(async (path: string) => ({ path, size: 0 })),
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

type TabsElement = HTMLElement & { shadowRoot: ShadowRoot }

function fileTabs(): TabsElement | null {
  return document.querySelector<TabsElement>('anth-file-tabs')
}

function tabEditors(): EditorElement[] {
  const host = fileTabs()
  return [...(host?.shadowRoot.querySelectorAll<EditorElement>('anth-file-editor') ?? [])]
}

function lastEditor(): EditorElement | null {
  return tabEditors().at(-1) ?? null
}

function entryButton(element: FilesElement, name: string): HTMLButtonElement | undefined {
  return [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.entry') ?? [])].find(
    (button) => button.textContent?.includes(name),
  )
}

async function previewFile(element: FilesElement, name: string): Promise<void> {
  entryButton(element, name)?.click()
  await settle(element)
}

async function previewAction(element: FilesElement, label: string): Promise<void> {
  const button = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.preview .actions button') ?? [])].find(
    (button) => button.textContent?.includes(label),
  )
  button?.click()
  await settleWindows()
}

async function openEditor(element: FilesElement, name: string): Promise<void> {
  await previewFile(element, name)
  await previewAction(element, 'Edit')
}

async function openCreate(element: FilesElement): Promise<void> {
  const create = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.fab-action') ?? [])].find(
    (button) => button.textContent?.includes('New file'),
  )
  create?.click()
  await settle(element)
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
  vi.unstubAllGlobals()
})

test('lists the root and navigates into a directory', async () => {
  const client = fakeClient()
  const element = await mount(client)
  expect(element.shadowRoot?.textContent).toContain('folder/')
  expect(element.shadowRoot?.textContent).toContain('note.txt')

  entryButton(element, 'folder')?.click()
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

  await previewFile(element, 'note.txt')

  expect(client.read).toHaveBeenCalledWith('/note.txt')
  expect(element.shadowRoot?.querySelector('pre')?.textContent).toBe('hello files')
  // The class drives the mobile swap: preview replaces the listing.
  expect(element.shadowRoot?.querySelector('.layout')?.classList.contains('preview-open')).toBe(true)
  const details = element.shadowRoot?.querySelector('.details')?.textContent?.replace(/\s+/g, ' ')
  expect(details).toContain('/note.txt')
  expect(details).toContain('File')
  expect(details).toContain('11 B')
})

test('previews images from the image endpoint', async () => {
  const client = fakeClient()
  const element = await mount(client)

  await previewFile(element, 'photo.png')

  const image = element.shadowRoot?.querySelector<HTMLImageElement>('.preview .image img')
  expect(image?.getAttribute('src')).toBe('/api/files/image?path=%2Fphoto.png')
  expect(client.read).not.toHaveBeenCalled()
  expect(
    [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.preview .actions button') ?? [])].some(
      (button) => button.textContent?.includes('Edit'),
    ),
  ).toBe(false)

  image?.dispatchEvent(new Event('error'))
  await settle(element)
  expect(element.shadowRoot?.querySelector('.placeholder')?.textContent).toContain(
    'Preview not available for this file.',
  )
})

test('shows a placeholder when a file cannot be previewed', async () => {
  const client = fakeClient()
  client.read = vi.fn().mockRejectedValue(new Error('not_text'))
  const element = await mount(client)

  await previewFile(element, 'note.txt')

  expect(element.shadowRoot?.querySelector('.placeholder')?.textContent).toContain(
    'Preview not available for this file.',
  )
  expect(element.shadowRoot?.querySelector('.error')).toBeNull()
  expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain('/note.txt')
})

test('opens a file in a global editor window', async () => {
  const client = fakeClient()
  const element = await mount(client)

  await openEditor(element, 'note.txt')

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

  await openCreate(element)

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

  await openCreate(element)

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

  await openEditor(element, 'note.txt')

  const window = lastWindow()
  window?.shadowRoot.querySelector<HTMLButtonElement>('.control-min')?.click()
  await window?.updateComplete
  expect(window?.minimized).toBe(true)

  await previewAction(element, 'Edit')
  expect(document.querySelectorAll('anth-window')).toHaveLength(1)
  expect(tabEditors()).toHaveLength(1)
  expect(lastWindow()?.minimized).toBe(false)
})

test('opens a second file as a tab in the same window', async () => {
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
  expect(document.querySelectorAll('anth-window')).toHaveLength(1)
  expect(tabEditors()).toHaveLength(2)
  expect(lastWindow()?.minimized).toBe(false)

  const tabs = [...(fileTabs()?.shadowRoot.querySelectorAll<HTMLButtonElement>('.tab-button') ?? [])]
  expect(tabs.map((tab) => tab.textContent?.trim())).toEqual(['note.txt', 'other.txt'])
  expect(tabs[0]?.title).toBe('/note.txt')
  expect(tabs[1]?.title).toBe('/other.txt')
  expect(tabs[1]?.getAttribute('aria-selected')).toBe('true')
})

test('switches and closes tabs without closing the window', async () => {
  const element = await mount(fakeClient())
  await openEditor(element, 'note.txt')
  const first = lastWindow()
  first?.shadowRoot.querySelector<HTMLButtonElement>('.control-min')?.click()
  await first?.updateComplete
  await openEditor(element, 'other.txt')

  const host = fileTabs()
  const tabs = [...(host?.shadowRoot.querySelectorAll<HTMLButtonElement>('.tab-button') ?? [])]
  tabs[0]?.click()
  await settleWindows()

  const editors = tabEditors()
  expect(editors[0]?.hidden).toBe(false)
  expect(editors[1]?.hidden).toBe(true)

  host?.shadowRoot.querySelectorAll<HTMLButtonElement>('.tab-close')[1]?.click()
  await settleWindows()
  expect(tabEditors()).toHaveLength(1)
  expect(lastWindow()).not.toBeNull()

  host?.shadowRoot.querySelector<HTMLButtonElement>('.tab-close')?.click()
  await settleWindows()
  expect(lastWindow()).toBeNull()
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

  await previewFile(element, 'note.txt')
  await previewAction(element, 'Delete')
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

  await openCreate(element)

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

test('offers create and upload from the FAB menu', async () => {
  const client = fakeClient()
  const element = await mount(client)

  const menu = element.shadowRoot?.querySelector('anth-fab-menu') as
    | (HTMLElement & { open: boolean; shadowRoot: ShadowRoot })
    | null
  expect(menu).not.toBeNull()
  menu?.shadowRoot.querySelector<HTMLButtonElement>('.fab')?.click()
  expect(menu?.open).toBe(true)

  const actions = [...(element.shadowRoot?.querySelectorAll<HTMLButtonElement>('.fab-action') ?? [])]
  expect(actions.map((button) => button.textContent?.trim())).toEqual([
    'New file',
    'Upload from device',
  ])

  const file = new File(['hello bytes'], 'upload.txt', { type: 'text/plain' })
  const picker = element.shadowRoot?.querySelector<HTMLInputElement>('.picker')
  Object.defineProperty(picker, 'files', { configurable: true, value: [file] })
  picker?.dispatchEvent(new Event('change'))
  await settle(element)
  await settleWindows()
  await settle(element)

  expect(client.upload).toHaveBeenCalledWith('/upload.txt', 'aGVsbG8gYnl0ZXM=', true)
  expect(element.shadowRoot?.querySelector('.notice')?.textContent).toContain('Uploaded upload.txt')
})

test('asks before replacing an uploaded file that exists', async () => {
  const client = fakeClient()
  const upload = vi
    .fn()
    .mockRejectedValueOnce(Object.assign(new Error('already exists'), { code: 'exists' }))
    .mockResolvedValueOnce({ path: '/upload.txt', size: 5 })
  client.upload = upload
  const element = await mount(client)
  vi.stubGlobal('confirm', vi.fn(() => true))

  const file = new File(['hello'], 'upload.txt', { type: 'text/plain' })
  const picker = element.shadowRoot?.querySelector<HTMLInputElement>('.picker')
  Object.defineProperty(picker, 'files', { configurable: true, value: [file] })
  picker?.dispatchEvent(new Event('change'))
  await settle(element)
  await settleWindows()
  await settle(element)

  expect(upload).toHaveBeenNthCalledWith(1, '/upload.txt', 'aGVsbG8=', true)
  expect(upload).toHaveBeenNthCalledWith(2, '/upload.txt', 'aGVsbG8=', false)
  expect(element.shadowRoot?.querySelector('.notice')?.textContent).toContain('Uploaded upload.txt')
})

test('saves an edited file and reloads the preview', async () => {
  const client = fakeClient()
  const element = await mount(client)

  await openEditor(element, 'note.txt')

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

test('shows a classic status bar with the cursor position', async () => {
  const element = await mount(fakeClient())

  await openCreate(element)

  const editor = lastEditor()
  const area = editor?.shadowRoot.querySelector<HTMLTextAreaElement>('textarea')
  if (area) {
    area.value = 'a\nbc'
    area.setSelectionRange(3, 3)
    area.dispatchEvent(new Event('input'))
  }
  await settleWindows()

  const status = editor?.shadowRoot.querySelector('.statusbar')?.textContent?.replace(/\s+/g, ' ')
  expect(status).toContain('Ln 2, Col 2')
  expect(status).toContain('Plain text')
  expect(status).toContain('UTF-8')
  expect(status).toContain('LF')
})

test('creates a file from the editor', async () => {
  const client = fakeClient()
  const element = await mount(client)

  await openCreate(element)

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
