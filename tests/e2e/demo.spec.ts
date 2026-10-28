import { expect, test } from '@playwright/test'

test('the demo network inventory renders the synthetic interfaces', async ({ page }) => {
  await page.goto('/#/network')
  const network = page.locator('anth-network')
  await expect(network.locator('.count')).toContainText('3 interfaces')
  await expect(network.getByText('eth0', { exact: true })).toBeVisible()
  await expect(network.getByText('wlan0', { exact: true })).toBeVisible()
})

test('a virtual interface can be created and deleted through the demo API', async ({
  request,
}) => {
  const created = await request.post('/api/network/create', {
    data: { name: 'e2e0', kind: 'dummy', up: true },
  })
  expect(created.ok()).toBeTruthy()
  expect((await created.json()).message).toContain('created dummy interface')

  const snapshot = await request.get('/api/network')
  expect(snapshot.ok()).toBeTruthy()
  const body = await snapshot.json()
  expect(body.interfaces.some((iface: { name: string }) => iface.name === 'e2e0')).toBe(true)

  const deleted = await request.post('/api/network/delete', { data: { name: 'e2e0' } })
  expect(deleted.ok()).toBeTruthy()
})

test('the firewall form adds a simulated rule', async ({ page }) => {
  await page.goto('/#/network/firewall')
  const firewall = page.locator('anth-firewall')
  await firewall.locator('input[type="number"]').fill('8443')
  await firewall.locator('input[maxlength="32"]').fill('e2e')
  await firewall.getByRole('button', { name: 'Add rule' }).click()
  await expect(firewall.locator('.count')).toContainText('1 rule')
  await expect(firewall.getByText('tcp/8443')).toBeVisible()
})

test('the demo file tree is a sandbox with seeded content', async ({ request }) => {
  const listing = await request.get('/api/files?path=/')
  expect(listing.ok()).toBeTruthy()
  const names = (await listing.json()).items.map((entry: { name: string }) => entry.name)
  expect(names).toContain('README.md')
  expect(names).toContain('docs')

  const created = await request.post('/api/files/write', {
    data: { path: 'e2e-note.txt', content: 'sandbox\n', create: true },
  })
  expect(created.ok()).toBeTruthy()
  const read = await request.get('/api/files/read?path=e2e-note.txt')
  expect((await read.json()).text).toBe('sandbox\n')
  const deleted = await request.post('/api/files/trash', { data: { path: 'e2e-note.txt' } })
  expect(deleted.ok()).toBeTruthy()
})

test('the demo process table is synthetic and mutable', async ({ request }) => {
  const snapshot = await request.get('/api/processes')
  const body = await snapshot.json()
  expect(body.items.some((process: { name: string }) => process.name === 'demo-worker')).toBe(true)

  const signal = await request.post('/api/processes/signal', {
    data: { pid: 1440, start_ticks: 144000, signal: 'term' },
  })
  expect(signal.ok()).toBeTruthy()
  const after = await (await request.get('/api/processes')).json()
  expect(after.items.some((process: { pid: number }) => process.pid === 1440)).toBe(false)

  const spawn = await request.post('/api/processes/spawn', {
    data: { program: '/bin/sleep', args: ['1'] },
  })
  expect(spawn.ok()).toBeTruthy()
})

test('the demo storage inventory is synthetic and mutable', async ({ page, request }) => {
  await page.goto('/#/storage')
  const storage = page.locator('anth-storage').first()
  await expect(storage.locator('.count')).toContainText('4 devices')
  await expect(storage.getByText('demo-data')).toBeVisible()

  const created = await request.post('/api/volumes/partition/create', {
    data: { disk: '/dev/sdb', kind: 'linux', start_bytes: 0, size_bytes: 2_147_483_648 },
  })
  expect(created.ok()).toBeTruthy()
  expect((await created.json()).message).toContain('/dev/sdb2')

  const formatted = await request.post('/api/volumes/format', {
    data: { partition: '/dev/sdb2', fs_type: 'ext4', label: 'e2e' },
  })
  expect(formatted.ok()).toBeTruthy()

  const resized = await request.post('/api/volumes/partition/resize', {
    data: { disk: '/dev/sdb', number: 2, size_bytes: 4_294_967_296 },
  })
  expect(resized.ok()).toBeTruthy()

  const deleted = await request.post('/api/volumes/partition/delete', {
    data: { disk: '/dev/sdb', number: 2 },
  })
  expect(deleted.ok()).toBeTruthy()
})

test('the demo accounts are synthetic and mutable', async ({ page, request }) => {
  await page.goto('/#/accounts')
  const accounts = page.locator('anth-accounts').first()
  await expect(accounts.locator('.count')).toContainText('3 users · 2 system')
  await expect(accounts.getByRole('heading', { name: 'demo', exact: true })).toBeVisible()

  const created = await request.post('/api/accounts/users', {
    data: { name: 'e2e-user', gecos: 'E2E User' },
  })
  expect(created.ok()).toBeTruthy()
  const user = (await (await request.get('/api/accounts')).json()).users.find(
    (entry: { name: string }) => entry.name === 'e2e-user',
  )
  expect(user.home).toBe('/home/e2e-user')
  expect(user.locked).toBe(true)

  const password = await request.post('/api/accounts/users/password', {
    data: { name: 'e2e-user', password: 'long-enough' },
  })
  expect(password.ok()).toBeTruthy()

  const group = await request.post('/api/accounts/groups', { data: { name: 'e2e-group' } })
  expect(group.ok()).toBeTruthy()
  const groupDeleted = await request.post('/api/accounts/groups/delete', {
    data: { name: 'e2e-group' },
  })
  expect(groupDeleted.ok()).toBeTruthy()

  const deleted = await request.post('/api/accounts/users/delete', {
    data: { name: 'e2e-user', remove_home: true },
  })
  expect(deleted.ok()).toBeTruthy()
  const after = await (await request.get('/api/accounts')).json()
  expect(after.users.some((entry: { name: string }) => entry.name === 'e2e-user')).toBe(false)
  expect(after.groups.some((entry: { name: string }) => entry.name === 'e2e-user')).toBe(false)
})

test('the demo terminal answers virtual commands', async ({ page }) => {
  await page.goto('/#/terminal')
  const output = await page.evaluate(async () => {
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
    const socket = new WebSocket(
      `${scheme}://${location.host}/api/terminal?cols=80&rows=24`,
    )
    let text = ''
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('terminal timeout')), 8000)
      socket.onerror = () => reject(new Error('terminal socket error'))
      socket.onmessage = (event) => {
        const message = JSON.parse(event.data as string) as {
          type: string
          data?: string
        }
        if (message.type === 'ready') {
          socket.send(JSON.stringify({ type: 'input', data: btoa('whoami\rexit\r') }))
        } else if (message.type === 'output' && message.data) {
          text += atob(message.data)
        } else if (message.type === 'exit') {
          clearTimeout(timer)
          socket.close()
          resolve()
        } else if (message.type === 'error') {
          clearTimeout(timer)
          reject(new Error('terminal error'))
        }
      }
    })
    return text
  })
  expect(output).toContain('Anthracite demo shell')
  expect(output).toContain('whoami')
  expect(output).toContain('demo')
  expect(output).toContain('logout')
})

test('the demo status reports the feature matrix', async ({ request }) => {
  const response = await request.get('/api/demo')
  expect(response.ok()).toBeTruthy()
  const body = await response.json()
  expect(body.mode).toBe('demo')
  expect(body.status.ttl_seconds).toBe(3600)
  expect(body.status.features).toMatchObject({
    network: true,
    firewall: true,
    files: true,
    processes: true,
    volumes: true,
    accounts: true,
    terminal: true,
  })
})

test('each demo cookie gets its own state', async ({ playwright, baseURL }) => {
  const first = await playwright.request.newContext({ baseURL })
  const second = await playwright.request.newContext({ baseURL })
  try {
    await first.get('/api/demo')
    await second.get('/api/demo')

    const created = await first.post('/api/accounts/groups', {
      data: { name: 'only-first' },
    })
    expect(created.ok()).toBeTruthy()
    const firstGroups = (await (await first.get('/api/accounts')).json()).groups.map(
      (group: { name: string }) => group.name,
    )
    const secondGroups = (await (await second.get('/api/accounts')).json()).groups.map(
      (group: { name: string }) => group.name,
    )
    expect(firstGroups).toContain('only-first')
    expect(secondGroups).not.toContain('only-first')
  } finally {
    await first.dispose()
    await second.dispose()
  }
})

test('reset restores the seeded demo state', async ({ request }) => {
  const created = await request.post('/api/files/write', {
    data: { path: 'reset-me.txt', content: 'temporary\n', create: true },
  })
  expect(created.ok()).toBeTruthy()

  const reset = await request.post('/api/demo/reset')
  expect(reset.ok()).toBeTruthy()
  expect((await reset.json()).status).toBe('reset')

  const after = await request.get('/api/files/read?path=reset-me.txt')
  expect(after.status()).toBe(404)
  const listing = await request.get('/api/files?path=/')
  expect(listing.ok()).toBeTruthy()
  expect((await listing.json()).items.some((entry: { name: string }) => entry.name === 'README.md')).toBe(
    true,
  )
})

test('the shell shows the demo banner and simulated chips', async ({ page }) => {
  await page.goto('/#/')
  await expect(page.locator('[data-demo]')).toContainText('Demo')
  await expect(page.locator('[data-demo]')).toContainText(
    'This session is simulated and private to this browser.',
  )
  await expect(page.locator('[data-demo] button')).toHaveText('Reset demo')
  await expect(page.locator('.sidebar .demo-chip')).toHaveCount(6)
})
