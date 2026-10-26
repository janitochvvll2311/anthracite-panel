import { afterEach, expect, test, vi } from 'vitest'

import { AnthApp } from './app'
import type { AuthClient, AuthState } from './auth-client'
import type { HostInfo } from './host-info-client'
import { DRAWER_CLOSE_EVENT } from './nav-drawer'
import type { AnthNavDrawer } from './nav-drawer'
import type { SettingsClient } from './settings-client'
import type { AnthTabPanel } from './tab-panel'

const SAMPLE: HostInfo = {
  hostname: 'turing',
  os: { name: 'Debian GNU/Linux', version: '13', kernel: '6.12.0', arch: 'x86_64' },
  uptime_seconds: 90_000,
  cpu_model: 'Intel Xeon',
  logical_cores: 8,
  load: { one: 1.5, five: 2.25, fifteen: 3 },
  memory: { total_kib: 8_388_608, available_kib: 6_291_456, swap_total_kib: 0, swap_free_kib: 0 },
  collected_at_ms: 1,
}

function authClient(state: AuthState): AuthClient {
  return {
    state: vi.fn().mockResolvedValue(state),
    login: vi.fn().mockResolvedValue(undefined),
    register: vi.fn().mockResolvedValue(undefined),
    logout: vi.fn().mockResolvedValue(undefined),
  }
}

const SETTINGS = {
  version: 1,
  socket_path: './anth.sock',
  state_dir: './state',
  server: { addr: '127.0.0.1:8080' },
  log: { level: 'info', format: 'human', out: 'stdout' },
  terminal: { enabled: false, shell: 'auto', max_sessions: 4 },
  process: { control: false, spawn: false },
  network: { control: false, manage: false },
  firewall: { control: false },
  volumes: { control: false, partition: false, format: false },
  accounts: { control: false, manage_passwords: false },
  files: { write: false, trash: false },
  auth: { enabled: false, username: 'admin', cookie_secure: false, bootstrap: 'auto' },
  plugins: { enabled: false },
}

function settingsClient(): SettingsClient {
  return {
    get: vi.fn().mockResolvedValue(SETTINGS),
    sessions: vi.fn().mockResolvedValue({ max: 10, sessions: [] }),
    update: vi.fn().mockResolvedValue({ restart_required: true }),
    restart: vi.fn().mockResolvedValue(undefined),
    reboot: vi.fn().mockResolvedValue(undefined),
    revokeOthers: vi.fn().mockResolvedValue(0),
  }
}

async function flush(times = 12): Promise<void> {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
}

function mountShell(state: AuthState): AnthApp {
  AnthApp.authClient = authClient(state)
  AnthApp.hostClient = {
    get: vi.fn().mockResolvedValue(SAMPLE),
    watch: vi.fn((_interval, handlers) => {
      handlers.onStatus?.('live')
      handlers.onSnapshot(SAMPLE)
      return () => {}
    }),
  }
  AnthApp.settingsClient = settingsClient()
  const element = document.createElement('anth-app') as AnthApp
  document.body.append(element)
  return element
}

const OPEN: AuthState = {
  required: false,
  registered: true,
  authenticated: true,
  bootstrap: 'closed',
  username: 'admin',
}

afterEach(() => {
  document.body.innerHTML = ''
  AnthApp.authClient = undefined
  AnthApp.hostClient = undefined
  AnthApp.settingsClient = undefined
  location.hash = ''
})

test('the frame keeps the header outside the scroll container', async () => {
  const element = mountShell(OPEN)
  await flush()
  const root = element.shadowRoot!

  const header = root.querySelector('header')
  const main = root.querySelector('main')
  expect(header).not.toBeNull()
  expect(main).not.toBeNull()
  expect(main!.contains(header!)).toBe(false)
  expect(header!.parentNode).toBe(root)
  expect(root.querySelectorAll('main')).toHaveLength(1)
  expect(root.querySelector('.sidebar')).not.toBeNull()
})

test('the sidebar, the tab strip and the drawer share the destinations', async () => {
  const element = mountShell(OPEN)
  await flush()
  const root = element.shadowRoot!

  const sidebar = root.querySelector('[data-sidebar]')!
  expect(sidebar.querySelectorAll('a')).toHaveLength(2)
  expect(sidebar.querySelector('a[data-destination="host"]')).not.toBeNull()
  expect(sidebar.querySelector('a[data-destination="settings"]')).not.toBeNull()

  const drawer = root.querySelector('anth-nav-drawer')!
  expect(drawer.querySelectorAll('a')).toHaveLength(2)

  const panel = root.querySelector('anth-tab-panel') as AnthTabPanel
  const tabs = panel.shadowRoot!.querySelectorAll('.tab')
  expect(tabs).toHaveLength(2)
  expect(tabs[0]!.textContent).toBe('Host')
  expect(tabs[1]!.textContent).toBe('Settings')
})

test('the menu opens the drawer and the drawer asks the shell to close it', async () => {
  const element = mountShell(OPEN)
  await flush()
  const root = element.shadowRoot!
  const drawer = root.querySelector('anth-nav-drawer') as AnthNavDrawer
  const menu = root.querySelector('.menu') as HTMLButtonElement

  expect(drawer.open).toBe(false)
  menu.click()
  expect(drawer.open).toBe(true)

  drawer.dispatchEvent(new CustomEvent(DRAWER_CLOSE_EVENT))
  expect(drawer.open).toBe(false)
})

test('switching a tab parks the inactive panel', async () => {
  const element = mountShell(OPEN)
  await flush()
  const root = element.shadowRoot!
  const panel = root.querySelector('anth-tab-panel') as AnthTabPanel

  expect(panel.active).toBe('host')
  expect(panel.querySelector('[data-tab="host"]')).not.toBeNull()
  expect(panel.querySelector('[data-tab="settings"]')).toBeNull()

  const tabs = panel.shadowRoot!.querySelectorAll<HTMLButtonElement>('.tab')
  tabs[1]!.click()
  await flush()

  expect(location.hash).toBe('#/settings')
  expect(panel.active).toBe('settings')
  expect(panel.querySelector('[data-tab="settings"]')).not.toBeNull()
  // The hidden panel is removed, which is what closes its stream.
  expect(panel.querySelector('[data-tab="host"]')).toBeNull()
})

test('an unauthenticated shell shows the registration form instead of the tabs', async () => {
  const element = mountShell({
    required: true,
    registered: false,
    authenticated: false,
    bootstrap: 'web',
    username: 'admin',
  })
  await flush()
  const root = element.shadowRoot!

  expect(root.querySelector('[data-auth] form[data-register]')).not.toBeNull()
  expect(root.querySelector('[data-shell]')!.hasAttribute('hidden')).toBe(true)
})
