import { afterEach, describe, expect, it } from 'vitest'

import './anth-nav-drawer'
import type { AnthNavDrawer } from './anth-nav-drawer'
import {
  ANTH_DRAWER_CLOSE_EVENT,
  type AnthDrawerCloseDetail,
} from './anth-nav-drawer'

async function settle(drawer: AnthNavDrawer): Promise<void> {
  for (let i = 0; i < 3; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await drawer.updateComplete
  }
}

async function mount(): Promise<{
  drawer: AnthNavDrawer
  frame: HTMLDialogElement
  reasons: string[]
}> {
  const drawer = document.createElement('anth-nav-drawer') as AnthNavDrawer
  drawer.innerHTML = `
    <nav>
      <a href="#/system">System</a>
      <a href="#/network">Network</a>
    </nav>
  `
  const reasons: string[] = []
  drawer.addEventListener(ANTH_DRAWER_CLOSE_EVENT, (event) => {
    reasons.push(
      (event as CustomEvent<AnthDrawerCloseDetail>).detail.reason,
    )
    drawer.open = false
  })
  document.body.append(drawer)
  drawer.open = true
  await settle(drawer)
  const frame = drawer.shadowRoot?.querySelector('dialog')
  if (!frame) throw new Error('the dialog was not rendered')
  return { drawer, frame, reasons }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthNavDrawer', () => {
  it('opens a native modal dialog with an accessible name', async () => {
    const { drawer, frame } = await mount()

    expect(drawer.open).toBe(true)
    expect(frame.open).toBe(true)
    expect(frame.getAttribute('aria-label')).toBe('Navigation')
    expect(
      frame.querySelector('.close')?.getAttribute('aria-label'),
    ).toBe('Close navigation')
  })

  it('reports the close button and closes when the owner clears open', async () => {
    const { drawer, frame, reasons } = await mount()

    frame.querySelector<HTMLButtonElement>('.close')?.click()
    await settle(drawer)

    expect(reasons).toEqual(['close'])
    expect(drawer.open).toBe(false)
    expect(frame.open).toBe(false)
  })

  it('reports Escape without letting the native close race the owner', async () => {
    const { drawer, frame, reasons } = await mount()

    frame.dispatchEvent(new Event('cancel', { cancelable: true, bubbles: true }))
    await settle(drawer)

    expect(reasons).toEqual(['escape'])
    expect(drawer.open).toBe(false)
    expect(frame.open).toBe(false)
  })

  it('reports a backdrop click but not a click inside the drawer', async () => {
    const { drawer, frame, reasons } = await mount()

    drawer.querySelector('nav')?.dispatchEvent(
      new MouseEvent('click', { bubbles: true, composed: true }),
    )
    expect(reasons).toEqual([])

    frame.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await settle(drawer)

    expect(reasons).toEqual(['backdrop'])
  })

  it('reports a navigation link so the shell can close the drawer', async () => {
    const { drawer, frame, reasons } = await mount()

    drawer.querySelector('a')?.dispatchEvent(
      new MouseEvent('click', { bubbles: true, composed: true }),
    )
    await settle(drawer)

    expect(reasons).toEqual(['link'])
    expect(drawer.open).toBe(false)
    expect(frame.open).toBe(false)
  })
})
