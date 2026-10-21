import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import type { AuthClient, SessionInfo } from '../lib/auth/auth-client'

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

/** An open session, so the shell renders the panels instead of the login. */
const openSession: SessionInfo = {
  enabled: false,
  authenticated: true,
  username: 'admin',
  access_expires_in: 900,
  refresh_expires_in: 3600,
}

/** Auth stub: the shell only needs the session read and the expiry hook. */
function authStub(session: SessionInfo = openSession): AuthClient {
  return {
    session: async () => session,
    refresh: async () => session,
    login: async () => session,
    logout: async () => {},
    changePassword: async () => {},
    onExpired: () => {},
    scheduleRefresh: () => {},
    cancelRefresh: () => {},
  } as unknown as AuthClient
}

async function mount(session: SessionInfo = openSession): Promise<AnthApp> {
  const element = document.createElement('anth-app') as AnthApp
  element.clientFactory = fakeClient
  element.authClient = authStub(session)
  document.body.append(element)
  await element.updateComplete
  // The session read resolves after the first render.
  for (let i = 0; i < 3; i++) await new Promise((r) => setTimeout(r, 0))
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

  it('opens the panel in a floating window for a panel route', async () => {
    const element = await mount()
    await navigate('#/panel/host-info')

    const desktop = element.shadowRoot?.querySelector('anth-desktop')
    const window = desktop?.shadowRoot?.querySelector('anth-window')
    expect(window?.getAttribute('heading')).toBe(PANELS[0].title())
  })

  it('opens the routed panel on a deep link', async () => {
    globalThis.location.hash = '#/panel/host-info'
    const element = await mount()

    const desktop = element.shadowRoot?.querySelector('anth-desktop')
    const windows = desktop?.shadowRoot?.querySelectorAll('anth-window')
    expect([...(windows ?? [])].map((w) => w.getAttribute('window-id'))).toEqual(
      ['host-info'],
    )
  })

  it('keeps the first window open when another panel is opened', async () => {
    const element = await mount()
    await navigate('#/panel/host-info')
    await navigate('#/panel/files')

    const windows = element.shadowRoot
      ?.querySelector('anth-desktop')
      ?.shadowRoot?.querySelectorAll('anth-window')
    expect([...(windows ?? [])].map((w) => w.getAttribute('window-id'))).toEqual(
      ['host-info', 'files'],
    )
  })

  it('restores the arrangement after a visit to the home page', async () => {
    const element = await mount()
    await navigate('#/panel/host-info')
    await navigate('#/panel/files')
    await navigate('#/')

    expect(element.shadowRoot?.querySelector('anth-desktop')).toBeNull()

    await navigate('#/panel/host-info')
    const windows = element.shadowRoot
      ?.querySelector('anth-desktop')
      ?.shadowRoot?.querySelectorAll('anth-window')
    expect([...(windows ?? [])].map((w) => w.getAttribute('window-id'))).toEqual(
      ['host-info', 'files'],
    )
  })

  it('opens the panel picked in the toolbar select', async () => {
    const element = await mount()
    await navigate('#/panel/host-info')

    const select = element.shadowRoot?.querySelector<HTMLSelectElement>(
      '.toolbar-select',
    )
    expect(select?.options).toHaveLength(PANELS.length)

    select!.value = 'files'
    select!.dispatchEvent(new Event('change'))
    await new Promise((r) => setTimeout(r, 0))

    const windows = element.shadowRoot
      ?.querySelector('anth-desktop')
      ?.shadowRoot?.querySelectorAll('anth-window')
    expect([...(windows ?? [])].map((w) => w.getAttribute('window-id'))).toEqual(
      ['host-info', 'files'],
    )
    expect(select?.value).toBe('')
  })

  it('mounts the feature element for a standalone window route', async () => {
    const element = await mount()
    await navigate('#/window/host-info')

    const mounted = element.shadowRoot?.querySelector('.panel-host')
      ?.firstElementChild
    expect(mounted?.localName).toBe('anth-host-info')
  })

  it('does not remount the standalone element when the route stays the same', async () => {
    const element = await mount()
    await navigate('#/window/host-info')
    const first = element.shadowRoot?.querySelector('.panel-host')
      ?.firstElementChild

    await navigate('#/window/host-info')
    const second = element.shadowRoot?.querySelector('.panel-host')
      ?.firstElementChild

    expect(second).toBe(first)
  })

  it('replaces the standalone element when the panel changes', async () => {
    const element = await mount()
    await navigate('#/window/host-info')
    await navigate('#/window/files')

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

  it('shows the login screen when there is no session', async () => {
    const anonymous: SessionInfo = {
      enabled: true,
      authenticated: false,
      username: 'admin',
      access_expires_in: 900,
      refresh_expires_in: 0,
    }
    const element = await mount(anonymous)

    const login = element.shadowRoot?.querySelector('anth-login')
    expect(login).not.toBeNull()
    expect(element.shadowRoot?.querySelector('.card')).toBeNull()
  })

  it('opens the panel once the login succeeds', async () => {
    const anonymous: SessionInfo = {
      enabled: true,
      authenticated: false,
      username: 'admin',
      access_expires_in: 900,
      refresh_expires_in: 0,
    }
    const element = await mount(anonymous)
    const login = element.shadowRoot?.querySelector('anth-login') as
      | HTMLElement
      | null
    expect(login).not.toBeNull()

    // The screen reports success with the fresh session.
    ;(login as unknown as { onAuthenticated: (s: SessionInfo) => void }).onAuthenticated(
      { ...openSession, enabled: true },
    )
    await element.updateComplete
    await new Promise((r) => setTimeout(r, 0))

    expect(element.shadowRoot?.querySelector('anth-login')).toBeNull()
    expect(element.shadowRoot?.querySelector('.card')).not.toBeNull()
  })

  it('offers a sign-out button only when auth is enabled', async () => {
    const element = await mount(openSession)
    expect(element.shadowRoot?.querySelector('.signout')).toBeNull()

    const secured = await mount({ ...openSession, enabled: true })
    expect(secured.shadowRoot?.querySelector('.signout')).not.toBeNull()
  })
})
