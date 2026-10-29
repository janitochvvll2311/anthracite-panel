import { afterEach, expect, test, vi } from 'vitest'

import './extensions'
import { HttpExtensionsClient } from './extensions-client'
import type { ExtensionView, ExtensionsClient, ExtensionsView } from './extensions-client'

function extension(overrides: Partial<ExtensionView> = {}): ExtensionView {
  return {
    id: 'org.anth.hello',
    alias: 'hello',
    name: 'Hello',
    version: '0.3.0',
    enabled: true,
    state: 'enabled',
    capabilities: ['host.hostname'],
    api: [{ method: 'GET', path: '/status' }],
    ui: [],
    ...overrides,
  }
}

function snapshot(overrides: Partial<ExtensionsView> = {}): ExtensionsView {
  return {
    enabled: true,
    install_from_web: true,
    extensions: [extension()],
    ...overrides,
  }
}

type ExtensionsElement = HTMLElement & {
  client: ExtensionsClient
  updateComplete: Promise<boolean>
}

async function settle(element: ExtensionsElement): Promise<void> {
  for (let index = 0; index < 6; index += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

function button(container: ParentNode, label: string): HTMLButtonElement {
  const found = [...container.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!found) throw new Error(`missing button: ${label}`)
  return found
}

async function mountExtensions(client: ExtensionsClient): Promise<ExtensionsElement> {
  const element = document.createElement('anth-extensions') as ExtensionsElement
  element.client = client
  document.body.append(element)
  await settle(element)
  return element
}

function fakeClient(value = snapshot()) {
  const get = vi.fn().mockResolvedValue(value)
  const install = vi.fn().mockResolvedValue({
    id: 'org.anth.hello',
    name: 'Hello',
    version: '0.3.0',
    capabilities: ['host.hostname'],
  })
  const enable = vi.fn().mockResolvedValue('extension enabled')
  const disable = vi.fn().mockResolvedValue('extension disabled')
  const remove = vi.fn().mockResolvedValue('extension removed')
  const approve = vi.fn().mockResolvedValue('extension approved')
  return {
    client: { get, install, enable, disable, remove, approve } as ExtensionsClient,
    get,
    install,
    enable,
    disable,
    remove,
    approve,
  }
}

afterEach(() => {
  document.body.replaceChildren()
})

test('lists the catalogue with policy, capabilities and actions', async () => {
  const feed = fakeClient(
    snapshot({
      extensions: [
        extension(),
        extension({
          id: 'org.anth.off',
          alias: 'off',
          name: 'Off',
          enabled: false,
          state: 'disabled',
          capabilities: [],
        }),
        extension({
          id: 'org.anth.bad',
          alias: undefined,
          name: 'Bad',
          enabled: false,
          state: 'error',
          error: 'the module digest does not match',
        }),
      ],
    }),
  )
  const element = await mountExtensions(feed.client)
  const root = element.shadowRoot!

  expect(root.querySelector('.count')?.textContent).toBe('3 extensions · active: 1')
  expect(root.querySelector('.policies')?.textContent).toContain('web install')
  expect(root.querySelectorAll('.extension')).toHaveLength(3)
  expect(root.querySelector('[part="install"]')).not.toBeNull()
  expect(root.querySelector('.extension')?.textContent).toContain('host.hostname')
  expect(root.textContent).toContain('the module digest does not match')
})

test('disables, enables and removes a extension with confirmation', async () => {
  const feed = fakeClient()
  const element = await mountExtensions(feed.client)
  const card = () => element.shadowRoot!.querySelector<HTMLElement>('.extension')!

  button(card(), 'Disable').click()
  await settle(element)
  expect(feed.disable).toHaveBeenCalledWith('org.anth.hello')

  button(card(), 'Remove').click()
  await element.updateComplete
  const confirm = card().querySelector<HTMLElement>('.confirm')!
  expect(confirm.textContent).toContain('Remove org.anth.hello and its grant?')
  button(confirm, 'Remove').click()
  await settle(element)
  expect(feed.remove).toHaveBeenCalledWith('org.anth.hello')
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
})

test('offers enable and approve for the matching states', async () => {
  const feed = fakeClient(
    snapshot({
      extensions: [
        extension({ enabled: false, state: 'disabled' }),
        extension({
          id: 'org.anth.bad',
          name: 'Bad',
          enabled: false,
          state: 'error',
          error: 'digest mismatch',
        }),
      ],
    }),
  )
  const element = await mountExtensions(feed.client)
  const cards = () => [...element.shadowRoot!.querySelectorAll<HTMLElement>('.extension')]

  button(cards()[0], 'Enable').click()
  await settle(element)
  expect(feed.enable).toHaveBeenCalledWith('org.anth.hello')

  button(cards()[1], 'Approve').click()
  await element.updateComplete
  const confirm = cards()[1].querySelector<HTMLElement>('.confirm')!
  button(confirm, 'Approve').click()
  await settle(element)
  expect(feed.approve).toHaveBeenCalledWith('org.anth.bad', true)
})

test('installs a bundle only after the trust box is checked', async () => {
  const feed = fakeClient(
    snapshot({ extensions: [], install_from_web: true }),
  )
  const element = await mountExtensions(feed.client)
  const form = element.shadowRoot?.querySelector<HTMLFormElement>('[part="install"]')!
  const file = {
    arrayBuffer: async () => new Uint8Array([1, 2, 3]).buffer,
    name: 'hello.zip',
  } as unknown as File
  const input = form.querySelector<HTMLInputElement>('input[type="file"]')!
  Object.defineProperty(input, 'files', { value: [file], configurable: true })

  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await element.updateComplete
  expect(feed.install).not.toHaveBeenCalled()
  expect(element.shadowRoot?.textContent).toContain('Confirm that you trust')

  const trust = form.querySelector<HTMLInputElement>('input[type="checkbox"]')!
  trust.checked = true
  trust.dispatchEvent(new Event('change'))
  await element.updateComplete
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)
  expect(feed.install).toHaveBeenCalledWith('AQID', false)
  expect(element.shadowRoot?.textContent).toContain('Installed org.anth.hello 0.3.0.')
})

test('hides the install form while extensions are disabled', async () => {
  const feed = fakeClient(snapshot({ enabled: false, extensions: [] }))
  const element = await mountExtensions(feed.client)
  expect(element.shadowRoot?.querySelector('[part="install"]')).toBeNull()
  expect(element.shadowRoot?.textContent).toContain('Extensions are disabled')
})

test('hides browser install when the switch is off and filters entries', async () => {
  const feed = fakeClient(
    snapshot({
      install_from_web: false,
      extensions: [
        extension(),
        extension({ id: 'org.anth.other', name: 'Other', alias: 'other' }),
      ],
    }),
  )
  const element = await mountExtensions(feed.client)
  const root = element.shadowRoot!
  expect(root.querySelector('[part="install"]')).toBeNull()
  expect(root.textContent).toContain('Browser installs are off')

  const filter = root.querySelector<HTMLInputElement>('input[type="search"]')!
  filter.value = 'other'
  filter.dispatchEvent(new Event('input'))
  await element.updateComplete
  expect(root.querySelectorAll('.extension')).toHaveLength(1)
  expect(root.querySelector('.extension')?.textContent).toContain('Other')
})

test('the http client calls every extension route', async () => {
  const calls: Array<{ url: string; body?: unknown }> = []
  const fetchImpl = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({
      url: String(url),
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    })
    if (String(url) === '/api/extensions' && init?.method !== 'POST') {
      return new Response(JSON.stringify(snapshot()), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }
    return new Response(JSON.stringify({ message: 'ok' }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  }) as unknown as typeof fetch
  const client = new HttpExtensionsClient(fetchImpl)

  await expect(client.get()).resolves.toEqual(snapshot())
  await expect(client.install('AQID', false)).resolves.toBeTruthy()
  await expect(client.enable('org.anth.hello')).resolves.toBe('ok')
  await expect(client.disable('org.anth.hello')).resolves.toBe('ok')
  await expect(client.remove('org.anth.hello')).resolves.toBe('ok')
  await expect(client.approve('org.anth.hello', true)).resolves.toBe('ok')

  expect(calls.map((call) => call.url)).toEqual([
    '/api/extensions',
    '/api/extensions/install',
    '/api/extensions/org.anth.hello/enable',
    '/api/extensions/org.anth.hello/disable',
    '/api/extensions/org.anth.hello/remove',
    '/api/extensions/org.anth.hello/approve',
  ])
  expect(calls.slice(1).map((call) => call.body)).toEqual([
    { bundle_base64: 'AQID', accept_capabilities: false },
    {},
    {},
    {},
    { accept_capabilities: true },
  ])
})
