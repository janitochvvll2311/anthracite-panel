import { beforeEach, describe, expect, it } from 'vitest'

import './anth-window'
import {
  ANTH_WINDOW_CLOSE_EVENT,
  ANTH_WINDOW_MINIMIZE_EVENT,
  ANTH_WINDOW_MOVE_EVENT,
  ANTH_WINDOW_RESIZE_EVENT,
  type AnthWindow,
} from './anth-window'
import { MIN_WIDTH } from '../lib/ui/windows'

async function mount(): Promise<AnthWindow> {
  const element = document.createElement('anth-window') as AnthWindow
  element.windowId = 'files'
  element.heading = 'Files'
  element.rect = { x: 40, y: 40, width: 600, height: 400 }
  document.body.append(element)
  await element.updateComplete
  return element
}

/** Dispatches a pointer event the way a real drag produces one. */
function pointer(
  target: EventTarget,
  type: string,
  x: number,
  y: number,
  extra: PointerEventInit = {},
): void {
  target.dispatchEvent(
    new PointerEvent(type, {
      bubbles: true,
      composed: true,
      button: 0,
      clientX: x,
      clientY: y,
      pointerId: 1,
      ...extra,
    }),
  )
}

beforeEach(() => {
  document.body.innerHTML = ''
})

describe('anth-window', () => {
  it('renders its heading and role', async () => {
    const element = await mount()
    expect(element.getAttribute('role')).toBe('dialog')
    expect(element.getAttribute('aria-label')).toBe('Files')
    expect(element.shadowRoot?.querySelector('.heading')?.textContent).toBe(
      'Files',
    )
  })

  it('positions itself from the rect', async () => {
    const element = await mount()
    const style = element.shadowRoot?.querySelector<HTMLElement>('.frame')?.style
    expect(style?.insetInlineStart).toBe('40px')
    expect(style?.inlineSize).toBe('600px')
  })

  it('reports a move while dragging the title bar', async () => {
    const element = await mount()
    let moved: { x: number; y: number } | undefined
    element.addEventListener(ANTH_WINDOW_MOVE_EVENT, (event) => {
      moved = (event as CustomEvent<{ rect: { x: number; y: number } }>).detail
        .rect
    })
    const title = element.shadowRoot?.querySelector('.title') as HTMLElement

    pointer(title, 'pointerdown', 100, 100)
    pointer(title, 'pointermove', 150, 130)

    expect(moved).toMatchObject({ x: 90, y: 70 })
  })

  it('reports a resize from the handle', async () => {
    const element = await mount()
    let resized: { width: number } | undefined
    element.addEventListener(ANTH_WINDOW_RESIZE_EVENT, (event) => {
      resized = (event as CustomEvent<{ rect: { width: number } }>).detail.rect
    })
    const handle = element.shadowRoot?.querySelector('.resize') as HTMLElement

    pointer(handle, 'pointerdown', 100, 100)
    pointer(handle, 'pointermove', 160, 140)

    expect(resized?.width).toBe(660)
  })

  it('does not resize below the minimum', async () => {
    const element = await mount()
    let resized: { width: number } | undefined
    element.addEventListener(ANTH_WINDOW_RESIZE_EVENT, (event) => {
      resized = (event as CustomEvent<{ rect: { width: number } }>).detail.rect
    })
    const handle = element.shadowRoot?.querySelector('.resize') as HTMLElement

    pointer(handle, 'pointerdown', 100, 100)
    pointer(handle, 'pointermove', -5000, -5000)

    expect(resized?.width).toBe(MIN_WIDTH)
  })

  it('stops following the pointer after it is released', async () => {
    const element = await mount()
    let moves = 0
    element.addEventListener(ANTH_WINDOW_MOVE_EVENT, () => {
      moves += 1
    })
    const title = element.shadowRoot?.querySelector('.title') as HTMLElement

    pointer(title, 'pointerdown', 100, 100)
    pointer(title, 'pointermove', 120, 120)
    pointer(title, 'pointerup', 120, 120)
    pointer(title, 'pointermove', 300, 300)

    expect(moves).toBe(1)
  })

  it('asks to minimize instead of minimizing itself', async () => {
    const element = await mount()
    let asked = false
    element.addEventListener(ANTH_WINDOW_MINIMIZE_EVENT, () => {
      asked = true
    })

    element.shadowRoot
      ?.querySelector<HTMLElement>('button[aria-label="Minimize"]')
      ?.click()

    expect(asked).toBe(true)
    expect(element.minimized).toBe(false)
  })

  it('asks to close instead of removing itself', async () => {
    const element = await mount()
    let asked = false
    element.addEventListener(ANTH_WINDOW_CLOSE_EVENT, () => {
      asked = true
    })

    element.shadowRoot
      ?.querySelector<HTMLElement>('button[aria-label="Close"]')
      ?.click()

    expect(asked).toBe(true)
    expect(document.body.contains(element)).toBe(true)
  })

  it('minimizes on a double click of the title bar', async () => {
    const element = await mount()
    let asked = 0
    element.addEventListener(ANTH_WINDOW_MINIMIZE_EVENT, () => {
      asked += 1
    })

    element.shadowRoot
      ?.querySelector('.title')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))

    expect(asked).toBe(1)
  })

  it('does not start a drag from a title-bar button', async () => {
    const element = await mount()
    let moves = 0
    element.addEventListener(ANTH_WINDOW_MOVE_EVENT, () => {
      moves += 1
    })
    const button = element.shadowRoot?.querySelector(
      'button[aria-label="Minimize"]',
    ) as HTMLElement

    pointer(button, 'pointerdown', 100, 100)
    pointer(button, 'pointermove', 200, 200)

    expect(moves).toBe(0)
  })

  it('only shows the pop-out button when it can pop out', async () => {
    const element = await mount()
    const find = (): Element | null | undefined =>
      element.shadowRoot?.querySelector('button[aria-label="Open in a separate window"]')
    expect(find()).toBeNull()

    element.canPopout = true
    await element.updateComplete
    expect(find()).not.toBeNull()
  })

  it('reads the can-popout attribute the desktop writes', async () => {
    const element = document.createElement('anth-window') as AnthWindow
    element.setAttribute('window-id', 'files')
    element.setAttribute('can-popout', '')
    document.body.append(element)
    await element.updateComplete

    expect(element.canPopout).toBe(true)
    expect(
      element.shadowRoot?.querySelector('button[aria-label="Open in a separate window"]'),
    ).not.toBeNull()
  })
})
