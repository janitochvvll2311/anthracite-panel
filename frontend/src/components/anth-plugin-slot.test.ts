import { beforeEach, describe, expect, it, vi } from 'vitest'

import './anth-plugin-slot'
import type { AnthPluginSlot } from './anth-plugin-slot'
import type { PluginsCatalogue } from '../lib/plugins/plugin-client'

const catalogue: PluginsCatalogue = {
  enabled: true,
  install_from_web: false,
  problems: [],
  plugins: [
    {
      id: 'org.anth.widget',
      name: 'Widget',
      version: '0.1.0',
      enabled: true,
      ui: [{ slot: 'dashboard.widget', tag: 'anth-widget', module: 'web/js/widget.js' }],
    },
  ],
}

function clientFor(modules: Record<string, string> = {}) {
  return {
    list: async () => catalogue,
    moduleUrl: (id: string, module: string) => {
      const source = modules[`${id}:${module}`]
      if (!source) throw new Error(`no module for ${id}:${module}`)
      return `data:text/javascript,${encodeURIComponent(source)}`
    },
  }
}

/** Waits for the slot's async refresh to settle. */
async function settle(element: AnthPluginSlot): Promise<void> {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
  await element.updateComplete
}

beforeEach(() => {
  document.body.replaceChildren()
})

describe('anth-plugin-slot', () => {
  it('mounts the element a plugin declared for its slot', async () => {
    const slot = document.createElement('anth-plugin-slot') as AnthPluginSlot
    slot.slotName = 'dashboard.widget'
    slot.catalogue = catalogue
    slot.client = clientFor({
      'org.anth.widget:web/js/widget.js':
        'customElements.define("anth-widget", class extends HTMLElement {})',
    }) as never
    document.body.append(slot)

    await settle(slot)

    const child = slot.querySelector('anth-widget')
    expect(child).not.toBeNull()
    expect(child?.tagName.toLowerCase()).toBe('anth-widget')
  })

  it('renders nothing for a slot no plugin declared', async () => {
    const slot = document.createElement('anth-plugin-slot') as AnthPluginSlot
    slot.slotName = 'panel.files.tab'
    slot.catalogue = catalogue
    slot.client = clientFor() as never
    document.body.append(slot)

    await settle(slot)

    expect(slot.children).toHaveLength(0)
  })

  it('does not mount a disabled plugin', async () => {
    const slot = document.createElement('anth-plugin-slot') as AnthPluginSlot
    slot.slotName = 'dashboard.widget'
    slot.catalogue = {
      ...catalogue,
      plugins: [{ ...catalogue.plugins[0], enabled: false }],
    }
    slot.client = clientFor({
      'org.anth.widget:web/js/widget.js':
        'customElements.define("anth-widget", class extends HTMLElement {})',
    }) as never
    document.body.append(slot)

    await settle(slot)

    expect(slot.children).toHaveLength(0)
  })

  it('skips a module that fails to load instead of throwing', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {})
    const slot = document.createElement('anth-plugin-slot') as AnthPluginSlot
    slot.slotName = 'dashboard.widget'
    slot.catalogue = catalogue
    slot.client = {
      list: async () => catalogue,
      moduleUrl: () => 'data:text/javascript,throw new Error("bad module")',
    } as never
    document.body.append(slot)

    await settle(slot)

    expect(slot.children).toHaveLength(0)
    expect(errors).toHaveBeenCalled()
    errors.mockRestore()
  })
})
