import { afterEach, describe, expect, it, vi } from 'vitest'

import './anth-fab'
import type { AnthFab } from './anth-fab'

function button(fab: AnthFab): HTMLButtonElement {
  const element = fab.shadowRoot?.querySelector('button')
  if (!element) throw new Error('the FAB button was not rendered')
  return element
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

describe('AnthFab', () => {
  it('renders the icon with an accessible label and tooltip', async () => {
    const fab = document.createElement('anth-fab') as AnthFab
    fab.icon = '＋'
    fab.label = 'Start process'
    document.body.append(fab)
    await fab.updateComplete

    expect(button(fab).getAttribute('aria-label')).toBe('Start process')
    expect(button(fab).getAttribute('title')).toBe('Start process')
    expect(button(fab).textContent).toContain('＋')
    expect(fab.hasAttribute('mobile-only')).toBe(true)
  })

  it('forwards a click to the panel that owns it', async () => {
    const fab = document.createElement('anth-fab') as AnthFab
    fab.label = 'Refresh'
    document.body.append(fab)
    await fab.updateComplete

    const clicked = vi.fn()
    fab.addEventListener('click', clicked)
    button(fab).click()

    expect(clicked).toHaveBeenCalledTimes(1)
  })

  it('does not emit a click while disabled', async () => {
    const fab = document.createElement('anth-fab') as AnthFab
    fab.label = 'Refresh'
    fab.disabled = true
    document.body.append(fab)
    await fab.updateComplete

    const clicked = vi.fn()
    fab.addEventListener('click', clicked)
    button(fab).click()

    expect(button(fab).disabled).toBe(true)
    expect(clicked).not.toHaveBeenCalled()
  })

  it('can opt out of the portrait-only rule', async () => {
    const fab = document.createElement('anth-fab') as AnthFab
    fab.mobileOnly = false
    document.body.append(fab)
    await fab.updateComplete

    expect(fab.hasAttribute('mobile-only')).toBe(false)
  })
})
