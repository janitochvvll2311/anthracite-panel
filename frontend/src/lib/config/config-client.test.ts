import { describe, expect, it, vi } from 'vitest'

import { HttpConfigClient } from './config-client'
import type { ConfigView } from './config-client'

function view(): ConfigView {
  return {
    socket_path: '/var/tmp/anth.sock',
    sudo: false,
    terminal: { shell: 'auto', max_sessions: 4 },
    process: { control: false, spawn: false, spawn_allow: [] },
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

function build(body: unknown, status = 200) {
  const fetchImpl = vi.fn(
    async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify(body), {
        status,
        headers: { 'Content-Type': 'application/json' },
      }),
  )
  return { client: new HttpConfigClient({ fetchImpl: fetchImpl as typeof fetch }), fetchImpl }
}

describe('HttpConfigClient', () => {
  it('reads the view', async () => {
    const { client } = build(view())
    const config = await client.read()
    expect(config.auth.username).toBe('admin')
  })

  it('sends only the patched section', async () => {
    const { client, fetchImpl } = build(view())
    await client.patch({ files: { write: true } })

    const [, init] = fetchImpl.mock.calls[0]
    expect(init?.method).toBe('PATCH')
    expect(JSON.parse(String(init?.body))).toEqual({ files: { write: true } })
  })

  it('surfaces the server detail on a rejected patch', async () => {
    const { client } = build({ error: 'authentication cannot be disabled' }, 400)
    await expect(client.patch({ auth: { enabled: false } })).rejects.toThrow(
      'authentication cannot be disabled',
    )
  })
})
