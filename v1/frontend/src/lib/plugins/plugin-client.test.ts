import { describe, expect, it, vi } from 'vitest'

import {
  HttpPluginClient,
  anchorsBySlot,
  loadSlot,
  type PluginsCatalogue,
} from './plugin-client'

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
      capabilities: [],
      api: [],
      ui: [{ slot: 'dashboard.widget', tag: 'anth-widget', module: 'web/js/widget.js' }],
    },
    {
      id: 'org.anth.off',
      name: 'Off',
      version: '0.1.0',
      enabled: false,
      capabilities: [],
      api: [],
      ui: [{ slot: 'dashboard.widget', tag: 'anth-off', module: 'web/js/off.js' }],
    },
    {
      id: 'org.anth.panel',
      name: 'Panel',
      version: '0.1.0',
      enabled: true,
      capabilities: [],
      api: [],
      ui: [{ slot: 'panel.files.tab', tag: 'anth-extra', module: 'web/js/extra.js' }],
    },
  ],
}

describe('anchorsBySlot', () => {
  it('lists only enabled plugins in the named slot', () => {
    const anchors = anchorsBySlot(catalogue, 'dashboard.widget')
    expect(anchors.map((entry) => entry.plugin)).toEqual(['org.anth.widget'])
  })

  it('separates slots, so a panel tab is not a dashboard card', () => {
    expect(anchorsBySlot(catalogue, 'panel.files.tab')).toHaveLength(1)
    expect(anchorsBySlot(catalogue, 'dashboard.widget')).toHaveLength(1)
  })

  it('is empty when plugins are switched off', () => {
    expect(anchorsBySlot({ ...catalogue, enabled: false }, 'dashboard.widget')).toEqual([])
  })

  it('is empty before the catalogue loads', () => {
    expect(anchorsBySlot(undefined, 'dashboard.widget')).toEqual([])
  })
})

describe('HttpPluginClient', () => {
  it('reads the catalogue from /api/plugins', async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify(catalogue)))
    const client = new HttpPluginClient({ fetchImpl: fetchImpl as unknown as typeof fetch })
    const result = await client.list()
    expect(result.plugins.map((plugin) => plugin.id)).toContain('org.anth.widget')
  })

  it('reports a failed read instead of returning an empty catalogue', async () => {
    const fetchImpl = vi.fn(async () => new Response('nope', { status: 500 }))
    const client = new HttpPluginClient({ fetchImpl: fetchImpl as unknown as typeof fetch })
    await expect(client.list()).rejects.toThrow('500')
  })

  it('uploads a bundle as the raw body and returns the warning', async () => {
    const result = {
      id: 'org.anth.widget',
      version: '0.1.0',
      name: 'Widget',
      warning: 'installed without a signature because `[plugins] require_signature = false`',
    }
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify(result)))
    const client = new HttpPluginClient({ fetchImpl: fetchImpl as unknown as typeof fetch })
    const file = new File([new Uint8Array([1, 2, 3])], 'widget.zip', {
      type: 'application/zip',
    })

    await expect(client.install(file)).resolves.toEqual(result)
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/plugins/install')
    expect(init.method).toBe('POST')
    expect(init.body).toBe(file)
    expect((init.headers as Record<string, string>)['content-type']).toBe(
      'application/zip',
    )
  })

  it('reports the server detail when an install is refused', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(JSON.stringify({ error: 'the bundle is not signed' }), {
          status: 403,
          headers: { 'content-type': 'application/json' },
        }),
    )
    const client = new HttpPluginClient({ fetchImpl: fetchImpl as unknown as typeof fetch })
    const file = new File(['zip'], 'widget.zip')

    await expect(client.install(file)).rejects.toThrow('the bundle is not signed')
  })

  it('builds an asset URL without duplicating the web prefix', () => {
    const client = new HttpPluginClient({ baseUrl: '' })
    expect(client.moduleUrl('org.anth.widget', 'web/js/widget.js')).toBe(
      '/api/plugins/org.anth.widget/web/js/widget.js',
    )
    // A manifest that omits the prefix still lands under web/.
    expect(client.moduleUrl('org.anth.widget', 'js/widget.js')).toBe(
      '/api/plugins/org.anth.widget/web/js/widget.js',
    )
  })

  it('encodes the id so it cannot split the path', () => {
    const client = new HttpPluginClient({ baseUrl: '' })
    expect(client.moduleUrl('org/../x', 'web/a.js')).toBe(
      '/api/plugins/org%2F..%2Fx/web/a.js',
    )
  })
})

describe('loadSlot', () => {
  it('imports the module and appends the declared tag', async () => {
    const calls: string[] = []
    const client = {
      list: async () => catalogue,
      moduleUrl: (id: string, module: string) => {
        calls.push(`${id}:${module}`)
        return `data:text/javascript,export default 1#${calls.length}`
      },
    }

    const elements = await loadSlot(client, catalogue, 'dashboard.widget')

    expect(calls).toEqual(['org.anth.widget:web/js/widget.js'])
    expect(elements.map((element) => element.tagName.toLowerCase())).toEqual([
      'anth-widget',
    ])
  })

  it('skips a plugin whose module fails to import', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {})
    const client = {
      list: async () => catalogue,
      moduleUrl: () => 'data:text/javascript,throw new Error("boom")',
    }

    const elements = await loadSlot(client, catalogue, 'dashboard.widget')

    expect(elements).toEqual([])
    expect(errors).toHaveBeenCalled()
    errors.mockRestore()
  })

  it('does nothing when plugins are off', async () => {
    const client = {
      list: async () => catalogue,
      moduleUrl: vi.fn(),
    }
    const elements = await loadSlot(client, { ...catalogue, enabled: false }, 'dashboard.widget')
    expect(elements).toEqual([])
    expect(client.moduleUrl).not.toHaveBeenCalled()
  })
})
