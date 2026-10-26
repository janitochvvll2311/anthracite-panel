import { afterEach, expect, test } from 'vitest'

import './window'
import type { AnthWindow, AnthWindowDock } from './window'

async function mount(): Promise<AnthWindow> {
  const dialog = document.createElement('anth-window') as AnthWindow
  dialog.label = 'Demo'
  dialog.innerHTML = '<p class="content">Body</p>'
  document.body.append(dialog)
  dialog.open = true
  await dialog.updateComplete
  return dialog
}

afterEach(() => {
  document.body.innerHTML = ''
})

test('maximizes and restores the frame', async () => {
  const dialog = await mount()
  const button = dialog.shadowRoot?.querySelector<HTMLButtonElement>('.control-max')
  expect(button).not.toBeNull()

  button?.click()
  await dialog.updateComplete
  expect(dialog.maximized).toBe(true)
  expect(dialog.hasAttribute('maximized')).toBe(true)

  button?.click()
  await dialog.updateComplete
  expect(dialog.maximized).toBe(false)
  expect(dialog.hasAttribute('maximized')).toBe(false)
})

test('parks a minimized window in the shared dock bar', async () => {
  const dialog = await mount()
  dialog.shadowRoot?.querySelector<HTMLButtonElement>('.control-min')?.click()
  await dialog.updateComplete
  expect(dialog.minimized).toBe(true)
  expect(dialog.hasAttribute('minimized')).toBe(true)

  const dock = document.querySelector<AnthWindowDock>('anth-window-dock')
  expect(dock).not.toBeNull()
  await dock?.updateComplete
  expect(dock?.shadowRoot?.textContent).toContain('Demo')

  dock?.shadowRoot?.querySelector<HTMLButtonElement>('.restore')?.click()
  await dialog.updateComplete
  expect(dialog.minimized).toBe(false)
  expect(dock?.shadowRoot?.querySelector('.bar')).toBeNull()
})

test('closing from the dock removes the chip', async () => {
  const dialog = await mount()
  dialog.shadowRoot?.querySelector<HTMLButtonElement>('.control-min')?.click()
  await dialog.updateComplete

  const dock = document.querySelector<AnthWindowDock>('anth-window-dock')
  await dock?.updateComplete
  dock?.shadowRoot?.querySelector<HTMLButtonElement>('.dismiss')?.click()
  await dialog.updateComplete

  expect(dialog.open).toBe(false)
  expect(dock?.shadowRoot?.querySelector('.bar')).toBeNull()
})
