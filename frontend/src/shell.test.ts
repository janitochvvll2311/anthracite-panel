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
  AnthApp.filesClient = {
    list: vi.fn().mockResolvedValue({ items: [], next: null }),
    read: vi.fn(),
    delete: vi.fn(),
    write: vi.fn(),
    trash: vi.fn(),
    trashList: vi.fn().mockResolvedValue({
      entries: [],
      total: 0,
      truncated: false,
      enabled: false,
      used_bytes: 0,
    }),
    restore: vi.fn(),
    emptyTrash: vi.fn(),
    archive: vi.fn(),
    archiveList: vi.fn(),
    extractArchive: vi.fn(),
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
  AnthApp.filesClient = undefined
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
  expect(element.classList.contains('auth-only')).toBe(false)
  const hostCard = root.querySelector('[data-host-card]')
  expect(hostCard?.querySelector('[data-refresh]')).toBeNull()
  expect(hostCard?.querySelector('anth-fab')).not.toBeNull()
  const signout = root.querySelector<HTMLButtonElement>('[data-signout]')
  expect(signout?.querySelector('anth-icon')).not.toBeNull()
  expect(signout?.querySelector('.btn-label')?.textContent).toContain('Sign out')
  const locale = root.querySelector('.locale')
  expect(locale?.querySelector('anth-icon')).not.toBeNull()
  expect(locale?.querySelector('select[data-locale]')).not.toBeNull()
})

test('the sidebar and the drawer share the sections, and the strip is bare there', async () => {
  const element = mountShell(OPEN)
  await flush()
  const root = element.shadowRoot!

  const sidebar = root.querySelector('[data-sidebar]')!
  expect(sidebar.querySelectorAll('a')).toHaveLength(3)
  expect(sidebar.querySelector('a[data-section="home"]')).not.toBeNull()
  expect(sidebar.querySelector('a[data-section="files"]')).not.toBeNull()
  expect(sidebar.querySelector('a[data-section="settings"]')).not.toBeNull()
  expect(sidebar.querySelector('a[data-section="home"] anth-icon')).not.toBeNull()

  const drawer = root.querySelector('anth-nav-drawer')!
  expect(drawer.querySelectorAll('a')).toHaveLength(3)

  const outer = root.querySelector('anth-tab-panel') as AnthTabPanel
  expect(outer.active).toBe('home')
  // The section strip is hidden: the sidebar already names the sections.
  expect((outer.shadowRoot!.querySelector('.strip') as HTMLElement).hidden).toBe(true)
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

test('the files section owns its own Files tab', async () => {
  const element = mountShell(OPEN)
  await flush()
  const root = element.shadowRoot!
  const outer = root.querySelector('anth-tab-panel') as AnthTabPanel

  location.hash = '#/files'
  window.dispatchEvent(new Event('hashchange'))
  await flush()

  expect(outer.active).toBe('files')
  expect(outer.querySelector('[data-tab="home"]')).toBeNull()
  const inner = outer.querySelector('anth-tab-panel') as AnthTabPanel
  expect(inner.active).toBe('files')
  expect([...inner.shadowRoot!.querySelectorAll('.tab')].map((tab) => tab.textContent)).toEqual([
    'Files',
  ])
  expect(
    inner.querySelector('[data-tab="files"]')?.querySelector('anth-files'),
  ).not.toBeNull()
})

test('the settings section owns a single General panel and parks Home', async () => {
  const element = mountShell(OPEN)
  await flush()
  const root = element.shadowRoot!
  const outer = root.querySelector('anth-tab-panel') as AnthTabPanel

  location.hash = '#/settings'
  window.dispatchEvent(new Event('hashchange'))
  await flush()

  expect(outer.active).toBe('settings')
  expect(outer.querySelector('[data-tab="home"]')).toBeNull()
  const inner = outer.querySelector('anth-tab-panel') as AnthTabPanel
  expect(inner.active).toBe('general')
  const tabs = inner.shadowRoot!.querySelectorAll<HTMLButtonElement>('.tab')
  expect([...tabs].map((tab) => tab.textContent)).toEqual(['General'])
  expect(inner.querySelector('[data-tab="general"]')?.querySelector('anth-settings')).not.toBeNull()
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
  expect(element.classList.contains('auth-only')).toBe(true)
})
