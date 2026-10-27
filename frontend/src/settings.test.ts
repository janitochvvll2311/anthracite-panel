import { afterEach, expect, test, vi } from 'vitest'

import './settings'
import type { Settings, SettingsClient } from './settings-client'

const settings: Settings = {
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
  auth: { enabled: true, username: 'admin', cookie_secure: false, bootstrap: 'auto' },
  plugins: { enabled: false },
}

function fakeClient(overrides: Partial<SettingsClient> = {}): SettingsClient {
  return {
    get: vi.fn().mockResolvedValue(settings),
    sessions: vi.fn().mockResolvedValue({
      max: 10,
      sessions: [{ sid: 'abcdef0123456789', expires: 1_800_000_000, current: true }],
    }),
    update: vi.fn().mockResolvedValue({ restart_required: true }),
    restart: vi.fn().mockResolvedValue(undefined),
    reboot: vi.fn().mockResolvedValue(undefined),
    revokeOthers: vi.fn().mockResolvedValue(1),
    ...overrides,
  }
}

type SettingsElement = HTMLElement & {
  client: SettingsClient
  updateComplete: Promise<boolean>
}

async function mount(client: SettingsClient): Promise<SettingsElement> {
  const element = document.createElement('anth-settings') as SettingsElement
  element.client = client
  document.body.append(element)
  await element.updateComplete
  await new Promise((resolve) => setTimeout(resolve, 0))
  await element.updateComplete
  return element
}

afterEach(() => {
  document.body.innerHTML = ''
})

test('renders every settings group under one General panel', async () => {
  const element = await mount(fakeClient())
  const text = element.shadowRoot?.textContent ?? ''

  expect(text).toContain('General')
  expect(text).toContain('admin')
  expect(text).toContain('Current')
  expect(text).toContain('Process control')
  expect(text).toContain('./anth.sock')
  expect(text).toContain('Restart panel')
})

test('a policy toggle writes the key and asks for a restart', async () => {
  const client = fakeClient()
  const element = await mount(client)

  const checkbox = element.shadowRoot?.querySelector(
    'input[type="checkbox"]',
  ) as HTMLInputElement
  checkbox.click()
  await element.updateComplete
  await new Promise((resolve) => setTimeout(resolve, 0))
  await element.updateComplete

  expect(client.update).toHaveBeenCalledWith('process.control', true)
  expect(element.shadowRoot?.textContent).toContain('Restart the panel to apply the changes.')
})

test('reloads only from the FAB', async () => {
  const client = fakeClient()
  const element = await mount(client)

  const fab = element.shadowRoot?.querySelector('anth-fab') as
    | (HTMLElement & { shadowRoot: ShadowRoot })
    | null
  expect(fab).not.toBeNull()
  const reload = [...(element.shadowRoot?.querySelectorAll('button') ?? [])].filter((button) =>
    button.textContent?.includes('Reload'),
  )
  expect(reload).toHaveLength(0)

  fab?.shadowRoot.querySelector<HTMLButtonElement>('button')?.click()
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(client.get).toHaveBeenCalledTimes(2)
})
