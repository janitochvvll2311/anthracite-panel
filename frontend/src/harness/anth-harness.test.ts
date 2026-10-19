import { afterEach, describe, expect, it } from 'vitest'
import './anth-harness'
import type { AnthHarness } from './anth-harness'

async function mount(): Promise<AnthHarness> {
  const element = document.createElement('anth-harness') as AnthHarness
  element.heading = 'demo-component'
  element.innerHTML = `
    <p slot="description">A slotted description.</p>
    <div slot="actions"><button type="button">Action</button></div>
    <section class="body">Body content.</section>
  `
  document.body.append(element)
  await element.updateComplete
  return element
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthHarness', () => {
  it('renders the heading and the default eyebrow', async () => {
    const element = await mount()
    const root = element.shadowRoot

    expect(root?.querySelector('h1')?.textContent).toBe('demo-component')
    expect(root?.querySelector('.eyebrow')?.textContent).toContain(
      'dev harness',
    )
  })

  it('projects description, actions and default content through slots', async () => {
    const element = await mount()
    const root = element.shadowRoot

    const description = root?.querySelector<HTMLSlotElement>(
      'slot[name="description"]',
    )
    const actions = root?.querySelector<HTMLSlotElement>(
      'slot[name="actions"]',
    )
    const defaultSlot = root?.querySelector<HTMLSlotElement>(
      'slot:not([name])',
    )

    expect(description?.assignedElements()[0]?.textContent?.trim()).toBe(
      'A slotted description.',
    )
    expect(actions?.assignedElements()[0]?.textContent?.trim()).toBe('Action')
    expect(defaultSlot?.assignedElements()[0]?.textContent?.trim()).toBe(
      'Body content.',
    )
  })

  it('supports a custom eyebrow and page width', async () => {
    const element = document.createElement('anth-harness') as AnthHarness
    element.heading = 'custom'
    element.eyebrow = 'custom eyebrow'
    element.width = '1400px'
    document.body.append(element)
    await element.updateComplete

    expect(
      element.shadowRoot?.querySelector('.eyebrow')?.textContent,
    ).toContain('custom eyebrow')
    expect(
      element.shadowRoot?.querySelector<HTMLElement>('.shell')?.getAttribute('style'),
    ).toContain('--harness-width: 1400px')
  })
})
