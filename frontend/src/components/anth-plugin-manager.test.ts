import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-plugin-manager'
import type { AnthPluginManager } from './anth-plugin-manager'
import {
  FakePluginClient,
  pluginSummary,
  pluginsCatalogue,
} from '../test/fake-plugin-client'
import { answerDialogs, dialogText, resetDialogs } from '../test/dialogs'

async function flush(element: AnthPluginManager): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  client: FakePluginClient,
): Promise<AnthPluginManager> {
  const element = document.createElement('anth-plugin-manager') as AnthPluginManager
  element.client = client
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

function fileInput(element: AnthPluginManager): HTMLInputElement {
  const input = element.shadowRoot?.querySelector<HTMLInputElement>(
    'input[type="file"]',
  )
  if (!input) throw new Error('the file input was not rendered')
  return input
}

function selectFile(element: AnthPluginManager, file: File): void {
  const input = fileInput(element)
  Object.defineProperty(input, 'files', { value: [file], configurable: true })
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

function submit(element: AnthPluginManager): void {
  const form = element.shadowRoot?.querySelector('form')
  if (!form) throw new Error('the install form was not rendered')
  form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
}

afterEach(() => {
  resetDialogs()
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('AnthPluginManager', () => {
  it('lists installed plugins and their frozen capabilities', async () => {
    const client = new FakePluginClient()
    client.emit(
      pluginsCatalogue({
        plugins: [
          pluginSummary({
            id: 'org.anth.wireguard',
            name: 'WireGuard',
            version: '0.2.0',
            capabilities: ['wg.status', 'wg.show'],
            ui: [{ slot: 'dashboard.widget', tag: 'anth-wireguard', module: 'web/w.js' }],
            api: [{ method: 'GET', path: '/status' }],
          }),
        ],
      }),
    )
    const element = await mount(client)
    const root = element.shadowRoot

    expect(client.list).toHaveBeenCalledTimes(1)
    expect(root?.textContent).toContain('WireGuard')
    expect(root?.textContent).toContain('org.anth.wireguard')
    expect(root?.textContent).toContain('v0.2.0')
    expect(root?.textContent).toContain('wg.status')
    expect(root?.textContent).toContain('UI: 1')
    expect(root?.textContent).toContain('API: 1')
  })

  it('shows problems reported by the catalogue', async () => {
    const client = new FakePluginClient()
    client.emit(
      pluginsCatalogue({
        problems: ['`org.anth.broken` has no grant and will not load'],
      }),
    )
    const element = await mount(client)
    expect(element.shadowRoot?.textContent).toContain('org.anth.broken')
  })

  it('explains why installation is unavailable', async () => {
    const client = new FakePluginClient()
    client.emit(pluginsCatalogue({ install_from_web: false }))
    const element = await mount(client)
    const root = element.shadowRoot

    expect(root?.querySelector('input[type="file"]')).toBeNull()
    expect(root?.textContent).toContain('install_from_web')
  })

  it('says when plugins are switched off', async () => {
    const client = new FakePluginClient()
    client.emit(pluginsCatalogue({ enabled: false, plugins: [] }))
    const element = await mount(client)

    expect(element.shadowRoot?.textContent).toContain('Plugins are switched off')
  })

  it('warns before installing and uploads the selected bundle', async () => {
    const client = new FakePluginClient()
    const element = await mount(client)
    const file = new File([new Uint8Array([1, 2, 3])], 'widget.zip', {
      type: 'application/zip',
    })
    selectFile(element, file)

    submit(element)
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(dialogText()).toBe('')
    expect(client.installCalls).toEqual([file])
    expect(element.shadowRoot?.textContent).toContain('Installed Widget 0.1.0')
    expect(client.list).toHaveBeenCalledTimes(2)
  })

  it('does not upload when the warning is cancelled', async () => {
    const client = new FakePluginClient()
    const element = await mount(client)
    selectFile(element, new File(['zip'], 'widget.zip'))

    submit(element)
    await flush(element)
    await answerDialogs('cancel')
    await flush(element)

    expect(client.installCalls).toEqual([])
  })

  it('surfaces the unverified-signature warning', async () => {
    const client = new FakePluginClient()
    client.warning =
      'installed without a signature because `[plugins] require_signature = false`'
    const element = await mount(client)
    selectFile(element, new File(['zip'], 'widget.zip'))

    submit(element)
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(element.shadowRoot?.querySelector('.warning')?.textContent).toContain(
      'without a signature',
    )
  })

  it('reports a refused install without clearing the selection', async () => {
    const client = new FakePluginClient()
    client.installError = new Error('the bundle is not signed')
    const element = await mount(client)
    selectFile(element, new File(['zip'], 'widget.zip'))

    submit(element)
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'not signed',
    )
  })
})
