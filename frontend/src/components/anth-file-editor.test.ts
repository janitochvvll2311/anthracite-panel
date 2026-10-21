import { afterEach, describe, expect, it, vi } from 'vitest'

/**
 * CodeMirror needs a real layout engine, so the editor is exercised through an
 * injected handle. That keeps the assertions on the component's own behaviour —
 * dirty flag, save precondition, error handling — and off the library's.
 */
const handle = {
  value: '',
  onChange: () => {},
  focus: vi.fn(),
  lineCount: () => handle.value.split('\n').length,
  destroy: vi.fn(),
  getValue: () => handle.value,
}

vi.mock('../lib/editor/codemirror-setup', () => ({
  createEditor: vi.fn(async (_parent, doc: string, _language, onChange: () => void) => {
    handle.value = doc
    handle.onChange = onChange
    return handle
  }),
}))

import './anth-file-editor'
import type { AnthFileEditor } from './anth-file-editor'

async function flush(element: AnthFileEditor): Promise<void> {
  for (let i = 0; i < 4; i += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  options: {
    path?: string
    contents?: string
    writable?: boolean
    save?: (contents: string) => Promise<{ modified_ms: number }>
  } = {},
): Promise<AnthFileEditor> {
  const element = document.createElement('anth-file-editor') as AnthFileEditor
  element.path = options.path ?? '/notes.txt'
  element.contents = options.contents ?? 'original\n'
  element.writable = options.writable ?? true
  element.save = options.save ?? (async () => ({ modified_ms: 1 }))
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

/** Simulates a keystroke: change the document, then notify the component. */
async function type(element: AnthFileEditor, text: string) {
  handle.value = text
  handle.onChange()
  await element.updateComplete
}

function badge(element: AnthFileEditor): string {
  return (
    element.shadowRoot?.querySelector('.badge')?.getAttribute('data-state') ?? ''
  )
}

function saveButton(element: AnthFileEditor): HTMLButtonElement {
  const button = element.shadowRoot?.querySelector<HTMLButtonElement>('button')
  if (!button) throw new Error('no save button')
  return button
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('anth-file-editor', () => {
  it('renders the path and starts clean', async () => {
    const element = await mount({ path: '/etc/hosts' })

    expect(element.shadowRoot?.textContent).toContain('/etc/hosts')
    expect(badge(element)).toBe('clean')
  })

  it('hands the loaded contents to the editor', async () => {
    await mount({ contents: 'hello\nworld\n' })

    expect(handle.value).toBe('hello\nworld\n')
  })

  it('disables Save until the buffer changes', async () => {
    const element = await mount()

    expect(saveButton(element).hasAttribute('disabled')).toBe(true)
  })

  it('marks the buffer dirty on an edit and updates the line count', async () => {
    const element = await mount({ contents: 'a\n' })

    // A trailing newline starts a fourth (empty) line, which is what an editor
    // shows too.
    await type(element, 'a\nb\nc\n')

    expect(badge(element)).toBe('dirty')
    expect(element.shadowRoot?.textContent).toContain('4 lines')
    expect(saveButton(element).hasAttribute('disabled')).toBe(false)
  })

  it('saves the buffer through the injected handler and returns to clean', async () => {
    const save = vi.fn(async () => ({ modified_ms: 42 }))
    const element = await mount({ save })
    await type(element, 'edited\n')

    saveButton(element).click()
    await flush(element)

    expect(save).toHaveBeenCalledWith('edited\n')
    expect(badge(element)).toBe('saved')
  })

  it('stays dirty and reports the message when the save fails', async () => {
    // A conflict is the interesting failure: the daemon refuses, and the text is
    // still only in the editor, so the buffer must not look saved.
    const save = vi.fn(async () => {
      throw new Error('changed on disk after it was opened')
    })
    const element = await mount({ save })
    await type(element, 'mine\n')

    saveButton(element).click()
    await flush(element)

    expect(badge(element)).toBe('dirty')
    expect(
      element.shadowRoot?.querySelector('[role="alert"]')?.textContent,
    ).toContain('changed on disk')
    expect(element.currentValue).toBe('mine\n')
  })

  it('does not call the handler when Save is pressed on a clean buffer', async () => {
    const save = vi.fn(async () => ({ modified_ms: 1 }))
    const element = await mount({ save })

    saveButton(element).click()
    await flush(element)

    expect(save).not.toHaveBeenCalled()
  })

  it('is read-only when writable is false', async () => {
    const element = await mount({ writable: false })

    expect(saveButton(element).hasAttribute('disabled')).toBe(true)
  })

  it('destroys the editor when removed from the document', async () => {
    const element = await mount()
    element.remove()

    expect(handle.destroy).toHaveBeenCalled()
  })
})
