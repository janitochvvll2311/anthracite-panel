import { afterEach, expect, test, vi } from 'vitest'

import './plugin-frame'
import type { PluginUiSlot, PluginView } from './plugins-client'

function plugin(overrides: Partial<PluginView> = {}): PluginView {
  return {
    id: 'org.anth.hello',
    alias: 'hello',
    name: 'Hello',
    version: '0.3.0',
    enabled: true,
    state: 'enabled',
    capabilities: [],
    api: [{ method: 'GET', path: '/status' }],
    ui: [],
    ...overrides,
  }
}

function slot(overrides: Partial<PluginUiSlot> = {}): PluginUiSlot {
  return {
    slot: 'settings.plugin',
    tag: 'anth-hello',
    module: 'web/hello.js',
    label: 'Hello',
    sha256: 'a'.repeat(64),
    ...overrides,
  }
}

type FrameElement = HTMLElement & {
  plugin: PluginView
  uiSlot: PluginUiSlot
  updateComplete: Promise<boolean>
}

async function mount(pluginValue = plugin()): Promise<FrameElement> {
  const element = document.createElement('anth-plugin-frame') as FrameElement
  element.plugin = pluginValue
  element.uiSlot = slot()
  document.body.append(element)
  await element.updateComplete
  return element
}

function message(
  frame: HTMLIFrameElement,
  data: Record<string, unknown>,
): MessageEvent {
  return new MessageEvent('message', {
    data: { anthPlugin: true, ...data },
    origin: 'null',
    source: frame.contentWindow,
  })
}

afterEach(() => {
  document.body.replaceChildren()
  vi.restoreAllMocks()
})

test('renders a sandboxed iframe with the declared module', async () => {
  const element = await mount()
  const iframe = element.shadowRoot?.querySelector('iframe')!
  expect(iframe.getAttribute('sandbox')).toBe('allow-scripts')
  expect(iframe.getAttribute('src')).toBe('/p/org.anth.hello/hello.js')
  expect(iframe.getAttribute('referrerpolicy')).toBe('no-referrer')
  expect(iframe.getAttribute('title')).toBe('Hello')
})

test('answers ready with the snapshot, locale and theme', async () => {
  const element = await mount()
  const iframe = element.shadowRoot?.querySelector('iframe')!
  const post = vi.fn()
  Object.defineProperty(iframe, 'contentWindow', {
    value: { postMessage: post },
    configurable: true,
  })
  window.dispatchEvent(message(iframe, { type: 'ready' }))
  expect(post).toHaveBeenCalledTimes(1)
  const payload = post.mock.calls[0][0]
  expect(payload.anthPluginHost).toBe(true)
  expect(payload.type).toBe('init')
  expect(payload.snapshot.plugin.id).toBe('org.anth.hello')
  expect(typeof payload.locale).toBe('string')
})

test('proxies a declared anchor and returns its body', async () => {
  const element = await mount()
  const iframe = element.shadowRoot?.querySelector('iframe')!
  const post = vi.fn()
  Object.defineProperty(iframe, 'contentWindow', {
    value: { postMessage: post },
    configurable: true,
  })
  const fetchImpl = vi.fn().mockResolvedValue(
    new Response(JSON.stringify({ status: 'ok' }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    }),
  )
  vi.stubGlobal('fetch', fetchImpl)

  window.dispatchEvent(
    message(iframe, {
      type: 'call',
      requestId: 4,
      method: 'GET',
      path: '/status',
    }),
  )
  await vi.waitFor(() => expect(fetchImpl).toHaveBeenCalled())
  await vi.waitFor(() => expect(post).toHaveBeenCalled())
  expect(fetchImpl).toHaveBeenCalledWith('/api/plugins/org.anth.hello/status', {
    method: 'GET',
    credentials: 'same-origin',
  })
  expect(post.mock.calls[0][0]).toMatchObject({
    type: 'result',
    requestId: 4,
    ok: true,
    status: 200,
    body: { status: 'ok' },
  })
})

test('refuses an anchor the plugin did not declare', async () => {
  const element = await mount()
  const iframe = element.shadowRoot?.querySelector('iframe')!
  const post = vi.fn()
  Object.defineProperty(iframe, 'contentWindow', {
    value: { postMessage: post },
    configurable: true,
  })
  const fetchImpl = vi.fn()
  vi.stubGlobal('fetch', fetchImpl)

  window.dispatchEvent(
    message(iframe, {
      type: 'call',
      requestId: 5,
      method: 'POST',
      path: '/secret',
    }),
  )
  await vi.waitFor(() => expect(post).toHaveBeenCalled())
  expect(fetchImpl).not.toHaveBeenCalled()
  expect(post.mock.calls[0][0]).toMatchObject({ ok: false, requestId: 5 })
})

test('shows a widget error and follows a resize message', async () => {
  const element = await mount()
  const iframe = element.shadowRoot?.querySelector('iframe')!
  Object.defineProperty(iframe, 'contentWindow', {
    value: { postMessage: vi.fn() },
    configurable: true,
  })
  window.dispatchEvent(
    message(iframe, { type: 'error', message: 'the widget broke' }),
  )
  await element.updateComplete
  expect(element.shadowRoot?.textContent).toContain('the widget broke')

  window.dispatchEvent(message(iframe, { type: 'resize', height: 300 }))
  await element.updateComplete
  const style = element.shadowRoot?.querySelector('iframe')?.getAttribute('style')
  expect(style).toContain('height: 308px')
})

test('ignores messages from another source or another origin', async () => {
  const element = await mount()
  const iframe = element.shadowRoot?.querySelector('iframe')!
  const post = vi.fn()
  Object.defineProperty(iframe, 'contentWindow', {
    value: { postMessage: post },
    configurable: true,
  })
  window.dispatchEvent(
    new MessageEvent('message', {
      data: { anthPlugin: true, type: 'ready' },
      origin: 'https://evil.example',
      source: iframe.contentWindow,
    }),
  )
  window.dispatchEvent(
    new MessageEvent('message', {
      data: { anthPlugin: true, type: 'ready' },
      origin: 'null',
      source: window,
    }),
  )
  expect(post).not.toHaveBeenCalled()
})
