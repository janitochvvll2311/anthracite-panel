import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthClient, SessionInfo } from '../lib/auth/auth-client'

import './anth-app'
import type { AnthApp } from './anth-app'
import { THEMES, panelsIn } from './panels'
import type { AnthThemePage } from '../components/anth-theme-page'
import type { AnthNavDrawer } from '../components/anth-nav-drawer'
import type { AnthTabPanel } from '../components/anth-tab-panel'
import { FakeAccountClient } from '../test/fake-account-client'
import { FakeFileClient } from '../test/fake-file-client'
import { FakeFirewallClient } from '../test/fake-firewall-client'
import { FakeHostInfoClient } from '../test/fake-host-info-client'
import { FakeNetworkClient } from '../test/fake-network-client'
import {
  FakePluginClient,
  pluginSummary,
  pluginsCatalogue,
} from '../test/fake-plugin-client'
import { FakeProcessClient } from '../test/fake-process-client'
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
    case 'processes':
      return new FakeProcessClient()
    case 'files':
      return new FakeFileClient()
    case 'volumes':
      return new FakeVolumeClient()
    case 'network':
      return new FakeNetworkClient()
    case 'firewall':
      return new FakeFirewallClient()
    case 'accounts':
      return new FakeAccountClient()
    case 'plugins':
      return new FakePluginClient()
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

function themePage(element: AnthApp): AnthThemePage | null {
  return element.shadowRoot?.querySelector<AnthThemePage>('anth-theme-page') ?? null
}

function tabPanel(element: AnthApp): AnthTabPanel | null {
  return (
    themePage(element)?.shadowRoot?.querySelector<AnthTabPanel>(
      'anth-tab-panel',
    ) ?? null
  )
}

function drawer(element: AnthApp): AnthNavDrawer | null {
  return (
    element.shadowRoot?.querySelector<AnthNavDrawer>('anth-nav-drawer') ?? null
  )
}

function drawerDialog(element: AnthApp): HTMLDialogElement | null {
  return (
    drawer(element)?.shadowRoot?.querySelector<HTMLDialogElement>('dialog') ??
    null
  )
}

function tabButton(
  element: AnthApp,
  id: string,
): HTMLButtonElement | null {
  return (
    tabPanel(element)?.shadowRoot?.querySelector<HTMLButtonElement>(
      `[role="tab"][data-tab="${id}"]`,
    ) ?? null
  )
}

async function settle(element: AnthApp): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
    const page = themePage(element)
    if (page) {
      await page.updateComplete
      const tabs = tabPanel(element)
      if (tabs) await tabs.updateComplete
    }
  }
}

async function mount(
  session: SessionInfo = openSession,
  pluginClient: FakePluginClient = new FakePluginClient(),
): Promise<AnthApp> {
  const element = document.createElement('anth-app') as AnthApp
  element.clientFactory = fakeClient
  element.authClient = authStub(session)
  element.pluginClient = pluginClient
  document.body.append(element)
  await settle(element)
  return element
}

async function navigate(element: AnthApp, hash: string): Promise<void> {
  globalThis.location.hash = hash
  globalThis.dispatchEvent(new HashChangeEvent('hashchange'))
  await settle(element)
}

/**
 * Runs `body` with `hashchange` listeners suppressed.
 *
 * A browser that loads `#/panel/x` parses the route before the element exists,
 * so no `hashchange` ever fires. happy-dom emits one when the hash is
 * assigned, which once hid a deep-link bug behind a passing assertion.
 */
async function withoutHashchange(body: () => Promise<void>): Promise<void> {
  const add = globalThis.addEventListener.bind(globalThis)
  const remove = globalThis.removeEventListener.bind(globalThis)
  globalThis.addEventListener = ((type: string, listener: EventListener) => {
    if (type !== 'hashchange') add(type, listener)
  }) as typeof globalThis.addEventListener
  globalThis.removeEventListener = ((type: string, listener: EventListener) => {
    if (type !== 'hashchange') remove(type, listener)
  }) as typeof globalThis.removeEventListener
  try {
    await body()
  } finally {
    globalThis.addEventListener = add
    globalThis.removeEventListener = remove
  }
}

beforeEach(() => {
  globalThis.location.hash = ''
  localStorage.clear()
})

afterEach(() => {
  document.body.innerHTML = ''
  globalThis.location.hash = ''
  localStorage.clear()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('AnthApp', () => {
  it('renders Home and one sidebar entry per theme', async () => {
    const element = await mount()
    const links = [
      ...(element.shadowRoot?.querySelectorAll<HTMLAnchorElement>(
        '.sidebar .nav-item',
      ) ?? []),
    ]

    expect(links).toHaveLength(THEMES.length + 1)
    expect(links[0].getAttribute('href')).toBe('#/')
    expect(links[0].textContent).toContain('Home')
    expect(links.slice(1).map((link) => link.dataset.theme)).toEqual(
      THEMES.map((theme) => theme.id),
    )
    expect(links.slice(1).map((link) => link.getAttribute('href'))).toEqual(
      THEMES.map((theme) => `#/panel/${panelsIn(theme.id)[0].id}`),
    )
  })

  it('marks Home as the current page on the home route', async () => {
    const element = await mount()
    const home = element.shadowRoot?.querySelector<HTMLAnchorElement>(
      '.sidebar .nav-item[href="#/"]',
    )

    expect(home?.getAttribute('aria-current')).toBe('page')
    expect(home?.hasAttribute('data-active')).toBe(true)
  })

  it('renders one home card per theme', async () => {
    const element = await mount()
    const cards = [
      ...(element.shadowRoot?.querySelectorAll<HTMLAnchorElement>('.card') ??
        []),
    ]

    expect(cards).toHaveLength(THEMES.length)
    expect(cards.map((card) => card.dataset.theme)).toEqual(
      THEMES.map((theme) => theme.id),
    )
  })

  it('mounts the dashboard plugin slot on Home', async () => {
    const element = await mount()

    expect(
      element.shadowRoot?.querySelector(
        'anth-plugin-slot[slot-name="dashboard.widget"]',
      ),
    ).not.toBeNull()
  })

  it('opens the routed theme page with its tab active', async () => {
    const element = await mount()
    await navigate(element, '#/panel/host-info')

    const host = element.shadowRoot?.querySelector('.theme-host')
    const page = themePage(element)
    const tabs = tabPanel(element)

    expect(host?.getAttribute('data-theme')).toBe('system')
    expect(page).not.toBeNull()
    expect(tabs?.active).toBe('host-info')
    expect(tabs?.tab('host-info')?.isConnected).toBe(true)
    expect(tabs?.tab('processes')?.isConnected).toBe(false)
    expect(
      tabs?.tab('host-info')?.firstElementChild?.localName,
    ).toBe('anth-host-info')
  })

  it('marks the routed theme active in the sidebar', async () => {
    const element = await mount()
    await navigate(element, '#/panel/firewall')
    const link = element.shadowRoot?.querySelector(
      '.nav-item[data-theme="network"]',
    )

    expect(link?.getAttribute('aria-current')).toBe('page')
    expect(link?.hasAttribute('data-active')).toBe(true)
  })

  it('opens the routed panel on a deep link', async () => {
    globalThis.location.hash = '#/panel/files'
    const element = await mount()

    expect(
      element.shadowRoot?.querySelector('.theme-host')?.getAttribute('data-theme'),
    ).toBe('storage')
    expect(tabPanel(element)?.active).toBe('files')
  })

  it('opens the routed panel on a deep link without a route change event', async () => {
    await withoutHashchange(async () => {
      globalThis.location.hash = '#/panel/files'
      const element = await mount()

      expect(tabPanel(element)?.active).toBe('files')
      expect(
        element.shadowRoot
          ?.querySelector('.theme-host')
          ?.getAttribute('data-theme'),
      ).toBe('storage')
    })
  })

  it('turns a tab click into a hash and remembers the theme tab', async () => {
    const element = await mount()
    await navigate(element, '#/panel/host-info')

    tabButton(element, 'processes')?.click()
    await settle(element)
    // happy-dom does not always emit hashchange from an assignment; the
    // browser does, and the test dispatches it explicitly.
    globalThis.dispatchEvent(new HashChangeEvent('hashchange'))
    await settle(element)

    expect(globalThis.location.hash).toBe('#/panel/processes')
    expect(localStorage.getItem('anth.theme.system')).toBe('processes')
    expect(tabPanel(element)?.active).toBe('processes')
  })

  it('links a theme to the panel it was last left on', async () => {
    const element = await mount()
    await navigate(element, '#/panel/firewall')
    await navigate(element, '#/')

    const link = element.shadowRoot?.querySelector(
      '.nav-item[data-theme="network"]',
    )
    expect(link?.getAttribute('href')).toBe('#/panel/firewall')
  })

  it('ignores a remembered panel that no longer exists', async () => {
    localStorage.setItem('anth.theme.system', 'does-not-exist')
    const element = await mount()

    const link = element.shadowRoot?.querySelector(
      '.nav-item[data-theme="system"]',
    )
    expect(link?.getAttribute('href')).toBe('#/panel/host-info')
  })

  it('adds a plugin tab inside the Network theme and links to it', async () => {
    const pluginClient = new FakePluginClient()
    pluginClient.moduleUrl = () => 'data:text/javascript,export default 1'
    pluginClient.current = pluginsCatalogue({
      plugins: [
        pluginSummary({
          id: 'org.anth.wireguard',
          name: 'WireGuard',
          ui: [
            {
              slot: 'panel.network.tab',
              tag: 'anth-wireguard-panel',
              module: 'web/wireguard.js',
              label: 'WireGuard',
            },
          ],
        }),
      ],
    })

    const element = await mount(openSession, pluginClient)
    await navigate(element, '#/panel/network')

    const tabs = tabPanel(element)
    expect(
      [...(tabs?.shadowRoot?.querySelectorAll('[role="tab"]') ?? [])].map(
        (tab) => tab.textContent?.trim(),
      ),
    ).toEqual(['Network', 'Firewall', 'WireGuard'])

    tabButton(element, 'plugin:org.anth.wireguard:anth-wireguard-panel')?.click()
    await settle(element)
    globalThis.dispatchEvent(new HashChangeEvent('hashchange'))
    await settle(element)

    expect(globalThis.location.hash).toBe(
      '#/panel/plugin%3Aorg.anth.wireguard%3Aanth-wireguard-panel',
    )
    expect(tabPanel(element)?.active).toBe(
      'plugin:org.anth.wireguard:anth-wireguard-panel',
    )
  })

  it('keeps the theme page when switching away and back', async () => {
    const element = await mount()
    await navigate(element, '#/panel/host-info')
    const first = themePage(element)

    await navigate(element, '#/panel/network')
    await navigate(element, '#/panel/host-info')

    expect(themePage(element)).toBe(first)
  })

  it('breaks the old pop-out route instead of mounting a standalone panel', async () => {
    const element = await mount()
    await navigate(element, '#/window/host-info')

    expect(element.shadowRoot?.querySelector('.theme-host')).toBeNull()
    expect(element.shadowRoot?.querySelector('anth-theme-page')).toBeNull()
    expect(
      element.shadowRoot?.querySelector('.unknown')?.textContent,
    ).toContain('No such page')
  })

  it('explains an unknown panel instead of mounting anything', async () => {
    const element = await mount()
    await navigate(element, '#/panel/does-not-exist')

    expect(element.shadowRoot?.querySelector('.theme-host')).toBeNull()
    expect(
      element.shadowRoot?.querySelector('.unknown')?.textContent,
    ).toContain('No such panel')
  })

  it('explains an unknown path', async () => {
    const element = await mount()
    await navigate(element, '#/nope')

    expect(
      element.shadowRoot?.querySelector('.unknown')?.textContent,
    ).toContain('No such page')
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

  it('returns to Home once the login succeeds', async () => {
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
    await settle(element)

    expect(element.shadowRoot?.querySelector('anth-login')).toBeNull()
    expect(element.shadowRoot?.querySelector('.card')).not.toBeNull()
  })

  it('offers a sign-out button only when auth is enabled', async () => {
    const element = await mount(openSession)
    expect(element.shadowRoot?.querySelector('.signout')).toBeNull()

    const secured = await mount({ ...openSession, enabled: true })
    expect(secured.shadowRoot?.querySelector('.signout')).not.toBeNull()
  })

  it('opens the navigation drawer from the menu button', async () => {
    const element = await mount()
    const menu = element.shadowRoot?.querySelector<HTMLButtonElement>('.menu')
    expect(menu?.getAttribute('aria-expanded')).toBe('false')

    menu?.click()
    await settle(element)

    const frame = drawer(element)
    const dialog = drawerDialog(element)
    expect(frame?.open).toBe(true)
    expect(dialog?.open).toBe(true)
    expect(menu?.getAttribute('aria-expanded')).toBe('true')
  })

  it('uses short theme labels inside the drawer', async () => {
    const element = await mount()
    const nav = drawer(element)?.querySelector('.drawer-nav')
    const labels = [...(nav?.querySelectorAll('.nav-label') ?? [])].map(
      (label) => label.textContent?.trim(),
    )

    expect(labels).toEqual([
      'Home',
      'System',
      'Data',
      'Network',
      'Access',
      'Extras',
    ])
  })

  it('closes the drawer when a navigation link is clicked', async () => {
    const element = await mount()
    element.shadowRoot?.querySelector<HTMLButtonElement>('.menu')?.click()
    await settle(element)

    const link = drawer(element)?.querySelector<HTMLAnchorElement>(
      '.drawer-nav .nav-item[data-theme="storage"]',
    )
    link?.click()
    await settle(element)

    expect(drawer(element)?.open).toBe(false)
    expect(drawerDialog(element)?.open).toBe(false)
    expect(
      element.shadowRoot
        ?.querySelector<HTMLButtonElement>('.menu')
        ?.getAttribute('aria-expanded'),
    ).toBe('false')
  })

  it('closes the drawer on Escape', async () => {
    const element = await mount()
    element.shadowRoot?.querySelector<HTMLButtonElement>('.menu')?.click()
    await settle(element)

    drawerDialog(element)?.dispatchEvent(
      new Event('cancel', { cancelable: true, bubbles: true }),
    )
    await settle(element)

    expect(drawer(element)?.open).toBe(false)
    expect(drawerDialog(element)?.open).toBe(false)
  })

  it('closes the drawer when the orientation becomes landscape', async () => {
    const listeners = new Set<() => void>()
    const query = {
      matches: true,
      addEventListener: (_type: string, listener: () => void) => {
        listeners.add(listener)
      },
      removeEventListener: (_type: string, listener: () => void) => {
        listeners.delete(listener)
      },
    } as unknown as MediaQueryList
    vi.stubGlobal('matchMedia', () => query)

    const element = await mount()
    element.shadowRoot?.querySelector<HTMLButtonElement>('.menu')?.click()
    await settle(element)
    expect(drawer(element)?.open).toBe(true)

    ;(query as { matches: boolean }).matches = false
    for (const listener of listeners) listener()
    await settle(element)

    expect(drawer(element)?.open).toBe(false)
  })
})
