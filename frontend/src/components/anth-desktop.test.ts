import { beforeEach, describe, expect, it } from 'vitest'

import './anth-desktop'
import type { AnthDesktop } from './anth-desktop'
import { ANTH_WINDOW_POPOUT_EVENT } from './anth-window'

const panels = [
  { id: 'processes', heading: 'Processes' },
  { id: 'files', heading: 'Files' },
]

function makePanel(id: string): HTMLElement {
  const element = document.createElement('div')
  element.dataset.panel = id
  return element
}

function mount(): AnthDesktop {
  const desktop = document.createElement('anth-desktop') as AnthDesktop
  desktop.panels = panels
  desktop.createPanel = makePanel
  document.body.append(desktop)
  return desktop
}

const settle = async (): Promise<void> => {
  for (let i = 0; i < 3; i++) await new Promise((r) => setTimeout(r, 0))
}

beforeEach(() => {
  localStorage.clear()
  document.body.innerHTML = ''
})

describe('anth-desktop', () => {
  it('opens a window and mounts its panel', async () => {
    const desktop = mount()
    desktop.open('processes')
    await settle()

    const windows = desktop.shadowRoot?.querySelectorAll('anth-window')
    expect(windows).toHaveLength(1)

    const slot = desktop.shadowRoot?.querySelector('.slot[data-slot="processes"]')
    expect(slot?.firstElementChild).toBeInstanceOf(HTMLElement)
    expect((slot?.firstElementChild as HTMLElement).dataset.panel).toBe('processes')
  })

  it('does not open a second copy of the same panel', async () => {
    const desktop = mount()
    desktop.open('processes')
    desktop.open('processes')
    await settle()

    expect(desktop.shadowRoot?.querySelectorAll('anth-window')).toHaveLength(1)
  })

  it('ignores a panel it does not know', async () => {
    const desktop = mount()
    desktop.open('nope')
    await settle()

    expect(desktop.shadowRoot?.querySelectorAll('anth-window')).toHaveLength(0)
  })

  it('passes can-popout to the window, so the button renders', async () => {
    const desktop = mount()
    desktop.open('processes')
    await settle()

    const window = desktop.shadowRoot?.querySelector('anth-window') as
      | (Element & { canPopout: boolean })
      | null
    expect(window?.hasAttribute('can-popout')).toBe(true)
    // The attribute is only useful if the child maps it to the property.
    expect(window?.canPopout).toBe(true)
  })

  it('hides the pop-out button when no-popout is set', async () => {
    const desktop = mount()
    desktop.noPopout = true
    desktop.open('processes')
    await settle()

    expect(
      desktop.shadowRoot?.querySelector('anth-window')?.hasAttribute('can-popout'),
    ).toBe(false)
  })

  it('detaches the body when minimized and restores the same element', async () => {
    const desktop = mount()
    desktop.open('processes')
    await settle()
    const before = desktop.shadowRoot?.querySelector(
      '.slot[data-slot="processes"]',
    )?.firstElementChild

    desktop.minimize('processes')
    await settle()
    atLeastOneOffscreen(desktop)
    expect(
      desktop.shadowRoot?.querySelector('.slot[data-slot="processes"]')
        ?.firstElementChild ?? null,
    ).toBeNull()

    desktop.restoreWindow('processes')
    await settle()
    const after = desktop.shadowRoot?.querySelector(
      '.slot[data-slot="processes"]',
    )?.firstElementChild

    expect(after).toBe(before)
  })

  it('keeps the layout across a remount', async () => {
    const first = mount()
    first.open('processes')
    await settle()
    first.minimize('processes')
    await settle()

    const stored = localStorage.getItem('anth.windows.main')
    expect(stored).toContain('processes')

    const second = mount()
    await settle()
    const window = second.shadowRoot?.querySelector('anth-window')
    expect(window?.hasAttribute('minimized')).toBe(true)
  })

  it('drops a stored window whose panel no longer exists', async () => {
    localStorage.setItem(
      'anth.windows.main',
      JSON.stringify([
        { id: 'gone', rect: { x: 0, y: 0, width: 400, height: 300 }, minimized: false, z: 1 },
        { id: 'files', rect: { x: 0, y: 0, width: 400, height: 300 }, minimized: false, z: 2 },
      ]),
    )
    const desktop = mount()
    await settle()

    const windows = [...(desktop.shadowRoot?.querySelectorAll('anth-window') ?? [])]
    expect(windows.map((w) => w.getAttribute('window-id'))).toEqual(['files'])
  })

  it('closes a window and removes its panel', async () => {
    const desktop = mount()
    desktop.open('processes')
    await settle()
    const body: Node | null | undefined =
      desktop.shadowRoot?.querySelector('.slot')?.firstElementChild

    desktop.close('processes')
    await settle()

    expect(desktop.shadowRoot?.querySelectorAll('anth-window')).toHaveLength(0)
    expect(body ? document.body.contains(body) : false).toBe(false)
  })

  it('raises the focused window above the others', async () => {
    const desktop = mount()
    desktop.open('processes')
    desktop.open('files')
    await settle()

    desktop.raise('processes')
    await settle()

    const windows = [...(desktop.shadowRoot?.querySelectorAll('anth-window') ?? [])]
    const processes = windows.find(
      (w) => w.getAttribute('window-id') === 'processes',
    ) as unknown as { z: number; active: boolean; hasAttribute: (n: string) => boolean }
    const files = windows.find(
      (w) => w.getAttribute('window-id') === 'files',
    ) as unknown as { z: number }
    // `z` is bound as a property, so the attribute stays empty by design.
    expect(processes.z).toBeGreaterThan(files.z)
    expect(processes?.hasAttribute('active')).toBe(true)
  })

  it('re-emits a pop-out request with the panel id', async () => {
    const desktop = mount()
    desktop.open('files')
    await settle()

    let seen = ''
    desktop.addEventListener(ANTH_WINDOW_POPOUT_EVENT, (event) => {
      seen = (event as CustomEvent<{ id: string }>).detail.id
    })
    desktop.shadowRoot
      ?.querySelector('anth-window')
      ?.dispatchEvent(new CustomEvent('anth-window-popout', { detail: { id: 'files' } }))
    expect(seen).toBe('files')
  })

  it('opens the initial panel on first render', async () => {
    const desktop = document.createElement('anth-desktop') as AnthDesktop
    desktop.panels = panels
    desktop.createPanel = makePanel
    desktop.initialPanel = 'files'
    document.body.append(desktop)
    await settle()

    const window = desktop.shadowRoot?.querySelector('anth-window')
    expect(window?.getAttribute('window-id')).toBe('files')
  })

  it('shows a hint when nothing is open', async () => {
    const desktop = mount()
    await settle()
    expect(desktop.shadowRoot?.querySelector('.empty')?.textContent).toContain(
      'No open windows',
    )
  })
})

/** The minimized element must be detached, not merely hidden. */
function atLeastOneOffscreen(desktop: AnthDesktop): void {
  const bodies = [...(desktop.shadowRoot?.querySelectorAll('.slot > *') ?? [])]
  expect(bodies).toHaveLength(0)
}

describe('anth-desktop · window chrome', () => {
  it('minimizes and detaches when the title-bar button is clicked', async () => {
    const desktop = mount()
    desktop.open('processes')
    await settle()

    const window = desktop.shadowRoot?.querySelector('anth-window')
    const button = window?.shadowRoot?.querySelector<HTMLElement>(
      'button[aria-label="Minimize"]',
    )
    button?.click()
    await settle()

    expect(window?.hasAttribute('minimized')).toBe(true)
    expect(
      desktop.shadowRoot?.querySelector('.slot[data-slot="processes"]')
        ?.firstElementChild ?? null,
    ).toBeNull()
  })

  it('closes when the title-bar button is clicked', async () => {
    const desktop = mount()
    desktop.open('files')
    await settle()

    const window = desktop.shadowRoot?.querySelector('anth-window')
    window?.shadowRoot
      ?.querySelector<HTMLElement>('button[aria-label="Close"]')
      ?.click()
    await settle()

    expect(desktop.shadowRoot?.querySelectorAll('anth-window')).toHaveLength(0)
  })
})
