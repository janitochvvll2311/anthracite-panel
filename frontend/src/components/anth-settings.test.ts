import { afterEach, describe, expect, it, vi } from 'vitest'

import './anth-settings'
import type { AnthSettings } from './anth-settings'
import type { ConfigPatch, ConfigView } from '../lib/config/config-client'

function view(): ConfigView {
  return {
    socket_path: '/var/tmp/anth.sock',
    sudo: false,
    terminal: { shell: 'auto', max_sessions: 4, token_set: false },
    process: { control: false },
    network: { control: false },
    firewall: { control: false },
    volumes: { control: false, mount_root: '/mnt' },
    accounts: {
      control: false,
      manage_passwords: false,
      allow_system: false,
      allow_privileged_groups: false,
      home_root: '/home',
      default_shell: '/bin/sh',
    },
    files: {
      root: '/',
      write: false,
      max_entries: 5000,
      max_preview_bytes: 1_048_576,
      max_upload_bytes: 67_108_864,
    },
    auth: {
      enabled: true,
      username: 'admin',
      password_set: true,
      access_token_secs: 900,
      refresh_token_secs: 43_200,
      cookie_secure: false,
    },
  }
}

async function mount(initial = view()) {
  const read = vi.fn(async () => initial)
  const patch = vi.fn(async (body: ConfigPatch) => ({
    ...initial,
    ...(body as Partial<ConfigView>),
  }))
  const element = document.createElement('anth-settings') as AnthSettings
  element.client = { read, patch } as unknown as NonNullable<AnthSettings['client']>
  document.body.append(element)
  await element.updateComplete
  await new Promise((resolve) => setTimeout(resolve, 0))
  await element.updateComplete
  return { element, read, patch }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthSettings', () => {
  it('renders one control per section', async () => {
    const { element } = await mount()
    const headings = [...(element.shadowRoot?.querySelectorAll('h2') ?? [])].map(
      (heading) => heading.textContent?.trim(),
    )
    expect(headings).toEqual([
      'Access',
      'Paths',
      'Limits',
      'Accounts',
      'Session',
    ])
  })

  it('sends nothing when no field changed', async () => {
    const { element, patch } = await mount()
    const save = [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
      (button) => button.textContent?.includes('Save'),
    )
    save?.click()
    await element.updateComplete

    expect(patch).not.toHaveBeenCalled()
  })

  it('sends only the section the operator touched', async () => {
    const { element, patch } = await mount()
    const checkboxes = [
      ...(element.shadowRoot?.querySelectorAll<HTMLInputElement>(
        'input[type="checkbox"]',
      ) ?? []),
    ]
    // The second checkbox is "Allow file writes".
    const writes = checkboxes[1]
    writes.checked = true
    writes.dispatchEvent(new Event('change'))
    await element.updateComplete

    const save = [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
      (button) => button.textContent?.includes('Save'),
    )
    save?.click()
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(patch).toHaveBeenCalledOnce()
    expect(patch.mock.calls[0][0]).toEqual({ files: { write: true } })
  })

  it('reports a rejected save without losing the draft', async () => {
    const { element } = await mount()
    const client = element.client as unknown as {
      patch: (patch: ConfigPatch) => Promise<ConfigView>
    }
    client.patch = vi.fn(async () => {
      throw new Error('files.root must be an absolute path')
    })

    const root = element.shadowRoot?.querySelector<HTMLInputElement>(
      'input[type="text"]',
    )
    if (root) {
      root.value = 'relative'
      root.dispatchEvent(new Event('change'))
      await element.updateComplete
    }

    const save = [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
      (button) => button.textContent?.includes('Save'),
    )
    save?.click()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete

    const status = element.shadowRoot?.querySelector('.status.error')
    expect(status?.textContent).toContain('absolute')
  })
})
