import { expect, test } from 'vitest'

import { HttpAuthClient } from './auth-client'
import { HttpHostInfoClient } from './host-info-client'
import { HttpSettingsClient } from './settings-client'

/**
 * Chrome throws "Illegal invocation" when `fetch` runs with anything other
 * than `window` (or `undefined`) as `this`. Node and happy-dom accept it, so
 * this fake reproduces the browser rule and guards against the regression.
 */
function browserFetch(body: unknown): typeof fetch {
  return function (this: unknown) {
    if (this !== undefined && this !== globalThis) {
      throw new TypeError("Failed to execute 'fetch' on 'Window': Illegal invocation")
    }
    return Promise.resolve(
      new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
      }),
    )
  } as typeof fetch
}

test('the auth client does not call fetch as a method', async () => {
  const client = new HttpAuthClient(
    browserFetch({
      required: true,
      registered: false,
      authenticated: false,
      bootstrap: 'web',
      username: 'admin',
    }),
  )

  await expect(client.state()).resolves.toMatchObject({
    bootstrap: 'web',
    username: 'admin',
  })
})

test('the host-info client does not call fetch as a method', async () => {
  const client = new HttpHostInfoClient(
    browserFetch({
      hostname: 'example',
      os: { name: 'Debian', version: '13', kernel: '6.12', arch: 'x86_64' },
      uptime_seconds: 1,
      cpu_model: 'Example CPU',
      logical_cores: 2,
      load: { one: 0, five: 0, fifteen: 0 },
      memory: {
        total_kib: 1024,
        available_kib: 512,
        swap_total_kib: 0,
        swap_free_kib: 0,
      },
      collected_at_ms: 1,
    }),
  )

  await expect(client.get()).resolves.toMatchObject({ hostname: 'example' })
})

test('the settings client does not call fetch as a method', async () => {
  const client = new HttpSettingsClient(
    browserFetch({
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
    }),
  )

  await expect(client.get()).resolves.toMatchObject({ version: 1 })
})
