import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import './anth-app'
import type { AnthApp } from './anth-app'
import { PANELS } from './panels'
import { FakeAccountClient } from '../test/fake-account-client'
import { FakeFileClient } from '../test/fake-file-client'
import { FakeFirewallClient } from '../test/fake-firewall-client'
import { FakeHostInfoClient } from '../test/fake-host-info-client'
import { FakeNetworkClient } from '../test/fake-network-client'
import { FakeVolumeClient } from '../test/fake-volume-client'

/** A transport stub that never opens a socket. */
const transportStub = {
  open: async () => ({ id: 1, size: { cols: 80, rows: 24 }, program: 'sh' }),
  write: () => {},
  resize: () => {},
  close: () => {},
  onData: () => () => {},
  onExit: () => () => {},
  onStatus: () => () => {},
}

function fakeClient(panel: { id: string }): unknown {
  switch (panel.id) {
    case 'host-info':
      return new FakeHostInfoClient()
    case 'files':
      return new FakeFileClient()
    case 'firewall':
      return new FakeFirewallClient()
    case 'network':
      return new FakeNetworkClient()
    case 'volumes':
      return new FakeVolumeClient()
    case 'accounts':
      return new FakeAccountClient()
    case 'terminal':
      return transportStub
    default:
      return undefined
  }
}

async function mount(): Promise<AnthApp> {
  const element = document.createElement('anth-app') as AnthApp
  element.clientFactory = fakeClient
  document.body.append(element)
  await element.updateComplete
  return element
}

async function navigate(hash: string): Promise<void> {
  globalThis.location.hash = hash
  globalThis.dispatchEvent(new HashChangeEvent('hashchange'))
  // Let the state change, the re-render and the post-render mount settle.
  for (let i = 0; i < 3; i++) await new Promise((r) => setTimeout(r, 0))
}

beforeEach(() => {
  globalThis.location.hash = ''
})

afterEach(() => {
  document.body.innerHTML = ''
  globalThis.location.hash = ''
})

describe('AnthApp', () => {
  it('renders one card per registered panel', async () => {
    const element = await mount()
    const cards = element.shadowRoot?.querySelectorAll('.card')

    expect(cards).toHaveLength(PANELS.length)
    expect([...(cards ?? [])].map((card) => card.getAttribute('href'))).toEqual(
      PANELS.map((panel) => `#/panel/${panel.id}`),
    )
  })

  it('mounts the feature element for a panel route', async () => {
    const element = await mount()
    await navigate('#/panel/host-info')

    const mounted = element.shadowRoot?.querySelector('.panel-host')
      ?.firstElementChild
    expect(mounted?.localName).toBe('anth-host-info')
  })

  it('does not remount the feature element when the route stays the same', async () => {
    const element = await mount()
    await navigate('#/panel/host-info')
    const first = element.shadowRoot?.querySelector('.panel-host')
      ?.firstElementChild

    await navigate('#/panel/host-info')
    const second = element.shadowRoot?.querySelector('.panel-host')
      ?.firstElementChild

    expect(second).toBe(first)
  })

  it('replaces the feature element when the panel changes', async () => {
    const element = await mount()
    await navigate('#/panel/host-info')
    await navigate('#/panel/files')

    const mounted = element.shadowRoot?.querySelector('.panel-host')
      ?.firstElementChild
    expect(mounted?.localName).toBe('anth-file-explorer')
  })

  it('explains an unknown panel instead of mounting anything', async () => {
    const element = await mount()
    await navigate('#/panel/does-not-exist')

    expect(element.shadowRoot?.querySelector('.panel-host')).toBeNull()
    expect(element.shadowRoot?.querySelector('.unknown')?.textContent).toContain(
      'No such panel',
    )
  })

  it('explains an unknown path', async () => {
    const element = await mount()
    await navigate('#/nope')

    expect(element.shadowRoot?.querySelector('.unknown')?.textContent).toContain(
      'No such page',
    )
  })

  it('offers the language selector with the current locale selected', async () => {
    const element = await mount()
    const select = element.shadowRoot?.querySelector('select')

    expect(select?.value).toBe('en')
    expect([...(select?.options ?? [])].map((option) => option.value)).toEqual([
      'en',
      'es',
    ])
  })
})
