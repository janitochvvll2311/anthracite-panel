import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-tab-panel'
import type { AnthTabPanel } from './anth-tab-panel'
import { ANTH_TAB_CHANGE_EVENT } from './anth-tab-panel'

/** A custom element that counts connections, to prove a parked tab is detached. */
class Probe extends HTMLElement {
  connects = 0
  disconnects = 0
  payload = ''

  connectedCallback(): void {
    this.connects += 1
  }

  disconnectedCallback(): void {
    this.disconnects += 1
  }
}

customElements.define('anth-tab-probe', Probe)

afterEach(() => {
  document.body.replaceChildren()
  localStorage.clear()
  vi.restoreAllMocks()
})

async function mount(
  tabs: Array<{ id: string; label?: string; disabled?: string }>,
  active?: string,
  storageKey?: string,
): Promise<AnthTabPanel> {
  const panel = document.createElement('anth-tab-panel') as AnthTabPanel
  if (active !== undefined) panel.active = active
  if (storageKey !== undefined) panel.storageKey = storageKey

  for (const tab of tabs) {
    const section = document.createElement('section')
    section.dataset.tab = tab.id
    if (tab.label) section.setAttribute('label', tab.label)
    if (tab.disabled) section.dataset.disabled = tab.disabled
    section.textContent = `body ${tab.id}`
    panel.append(section)
  }

  document.body.append(panel)
  await panel.updateComplete
  await panel.updateComplete
  return panel
}

function tabButtons(panel: AnthTabPanel): HTMLButtonElement[] {
  return [
    ...(panel.shadowRoot?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ??
      []),
  ]
}

function visibleIds(panel: AnthTabPanel): string[] {
  return [...panel.children].map((child) => (child as HTMLElement).dataset.tab!)
}

describe('anth-tab-panel', () => {
  it('renders one button per declared tab', async () => {
    const panel = await mount([
      { id: 'users', label: 'Users' },
      { id: 'groups', label: 'Groups' },
    ])

    expect(tabButtons(panel).map((button) => button.textContent?.trim())).toEqual(
      ['Users', 'Groups'],
    )
  })

  it('falls back to the first enabled tab when active is empty', async () => {
    const panel = await mount([
      { id: 'a', label: 'A', disabled: 'needs root' },
      { id: 'b', label: 'B' },
    ])

    expect(panel.active).toBe('b')
    expect(visibleIds(panel)).toEqual(['b'])
  })

  it('keeps only the active tab in the DOM', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ])

    expect(visibleIds(panel)).toEqual(['a'])
    expect(panel.children).toHaveLength(1)
  })

  it('parks a tab that was opened and restores it with its state', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ])

    const first = panel.children[0] as HTMLElement
    const probe = document.createElement('anth-tab-probe') as Probe
    probe.payload = 'typed'
    first.append(probe)
    const connectsAfterFirstMount = probe.connects

    tabButtons(panel)[1].click()
    await panel.updateComplete
    await panel.updateComplete

    expect(visibleIds(panel)).toEqual(['b'])
    expect(first.isConnected).toBe(false)
    expect(probe.disconnects).toBe(1)

    tabButtons(panel)[0].click()
    await panel.updateComplete
    await panel.updateComplete

    // The same element comes back: not a rebuild, which is what preserves state.
    expect(panel.children[0]).toBe(first)
    expect((panel.children[0] as HTMLElement).querySelector('anth-tab-probe')).toBe(
      probe,
    )
    expect(probe.payload).toBe('typed')
    expect(probe.connects).toBe(connectsAfterFirstMount + 1)
  })

  it('keeps an unvisited tab out of the document', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ])

    // Tab b was parsed with the rest of the markup, so it exists — it did
    // connect once before the panel parked it — but it is not in the document
    // and does not render. That is the saving this component offers.
    expect(panel.children).toHaveLength(1)
    expect(visibleIds(panel)).toEqual(['a'])
    expect(panel.querySelector('[data-tab="b"]')).toBeNull()
  })

  it('refuses a disabled tab', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B', disabled: 'needs root' },
    ])

    const disabled = tabButtons(panel)[1]
    expect(disabled.disabled).toBe(true)
    expect(disabled.getAttribute('title')).toBe('needs root')

    disabled.click()
    await panel.updateComplete
    expect(panel.active).toBe('a')
  })

  it('emits a change event with the new id', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ])

    const seen: string[] = []
    panel.addEventListener(ANTH_TAB_CHANGE_EVENT, (event) => {
      seen.push((event as CustomEvent<{ id: string }>).detail.id)
    })

    tabButtons(panel)[1].click()
    expect(seen).toEqual(['b'])
  })

  it('moves with the arrow keys and skips disabled tabs', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B', disabled: 'needs root' },
      { id: 'c', label: 'C' },
    ])

    const strip = panel.shadowRoot!.querySelector('.strip') as HTMLElement
    strip.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }),
    )
    await panel.updateComplete

    expect(panel.active).toBe('c')

    strip.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }),
    )
    await panel.updateComplete
    expect(panel.active).toBe('a')

    strip.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'End', bubbles: true }),
    )
    await panel.updateComplete
    expect(panel.active).toBe('c')
  })

  it('remembers the selection under storage-key', async () => {
    const first = await mount(
      [
        { id: 'a', label: 'A' },
        { id: 'b', label: 'B' },
      ],
      undefined,
      'accounts',
    )
    tabButtons(first)[1].click()
    await first.updateComplete
    expect(localStorage.getItem('anth.tabs.accounts')).toBe('b')

    first.remove()

    const second = await mount(
      [
        { id: 'a', label: 'A' },
        { id: 'b', label: 'B' },
      ],
      undefined,
      'accounts',
    )
    expect(second.active).toBe('b')
    expect(visibleIds(second)).toEqual(['b'])
  })

  it('restores a stored choice over the markup default', async () => {
    localStorage.setItem('anth.tabs.accounts', 'b')

    const panel = await mount(
      [
        { id: 'a', label: 'A' },
        { id: 'b', label: 'B' },
      ],
      'a',
      'accounts',
    )

    expect(panel.active).toBe('b')
    expect(visibleIds(panel)).toEqual(['b'])
  })

  it('ignores a stored id that no longer exists', async () => {
    localStorage.setItem('anth.tabs.accounts', 'gone')

    const panel = await mount(
      [
        { id: 'a', label: 'A' },
        { id: 'b', label: 'B' },
      ],
      undefined,
      'accounts',
    )

    expect(panel.active).toBe('a')
  })

  it('survives a storage API that throws', async () => {
    const panel = await mount([{ id: 'a', label: 'A' }], undefined, 'accounts')
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })

    expect(() => {
      panel.active = 'a'
      panel.storageKey = 'other'
    }).not.toThrow()
  })

  it('reaches a parked tab through tab()', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ])

    const parked = panel.tab('b')
    expect(parked?.isConnected).toBe(false)
    parked!.setAttribute('label', 'Renamed')
    // The observer that notices a parked tab's attribute change is a
    // microtask, so the strip rebuilds one tick later.
    await new Promise((resolve) => setTimeout(resolve, 0))
    await panel.updateComplete

    expect(tabButtons(panel).map((button) => button.textContent?.trim())).toEqual(
      ['A', 'Renamed'],
    )
    expect(panel.tab('missing')).toBeUndefined()
  })

  it('rebuilds the strip when a tab turns disabled', async () => {
    const panel = await mount([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ])

    // Tab b is parked, so it is not reachable through the DOM.
    panel.tab('b')!.setAttribute('data-disabled', 'root')
    await new Promise((resolve) => setTimeout(resolve, 0))
    await panel.updateComplete

    expect(tabButtons(panel)[1].disabled).toBe(true)
    expect(tabButtons(panel)[1].getAttribute('title')).toBe('root')
  })

  it('ignores a child without an id', async () => {
    const panel = await mount([{ id: 'a', label: 'A' }])

    const stray = document.createElement('p')
    stray.textContent = 'not a tab'
    panel.append(stray)
    await new Promise((resolve) => setTimeout(resolve, 0))
    await panel.updateComplete

    expect(tabButtons(panel)).toHaveLength(1)
    expect(stray.isConnected).toBe(true)
  })

  it('adds a tab that appears later', async () => {
    const panel = await mount([{ id: 'a', label: 'A' }])

    const late = document.createElement('section')
    late.dataset.tab = 'late'
    late.setAttribute('label', 'Late')
    panel.append(late)
    await panel.updateComplete
    await panel.updateComplete

    expect(tabButtons(panel)).toHaveLength(2)
    expect(panel.active).toBe('a')
    expect(late.isConnected).toBe(false)
  })
})
