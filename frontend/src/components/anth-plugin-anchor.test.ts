import { afterEach, describe, expect, it } from 'vitest'

import './anth-plugin-anchor'
import type { AnthPluginAnchor } from './anth-plugin-anchor'
import type { PluginUiAnchor } from '../lib/plugins/plugin-client'

const anchor: PluginUiAnchor = {
  slot: 'panel.network.tab',
  tag: 'anth-test-anchor',
  module: 'web/test.js',
  label: 'Test',
}

async function settle(element: AnthPluginAnchor): Promise<void> {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthPluginAnchor', () => {
  it('imports the module and appends the declared tag with its slot', async () => {
    const source = encodeURIComponent(
      "class TestAnchor extends HTMLElement {}; customElements.define('anth-test-anchor', TestAnchor)",
    )
    const element = document.createElement('anth-plugin-anchor') as AnthPluginAnchor
    element.plugin = 'org.anth.test'
    element.anchor = anchor
    element.client = {
      list: async () => ({ enabled: true, install_from_web: false, plugins: [], problems: [] }),
      install: async () => ({ id: '', version: '', name: '' }),
      moduleUrl: () => `data:text/javascript,${source}`,
    }
    document.body.append(element)
    await settle(element)

    const mounted = element.querySelector('anth-test-anchor')
    expect(mounted).not.toBeNull()
    expect(mounted?.getAttribute('data-slot')).toBe('panel.network.tab')
    expect(element.shadowRoot?.querySelector('.error')).toBeNull()
  })

  it('reports a module that cannot be imported', async () => {
    const element = document.createElement('anth-plugin-anchor') as AnthPluginAnchor
    element.plugin = 'org.anth.test'
    element.anchor = anchor
    element.client = {
      list: async () => ({ enabled: true, install_from_web: false, plugins: [], problems: [] }),
      install: async () => ({ id: '', version: '', name: '' }),
      moduleUrl: () => 'data:text/javascript,this is not javascript',
    }
    document.body.append(element)
    await settle(element)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'could not be loaded',
    )
  })
})
