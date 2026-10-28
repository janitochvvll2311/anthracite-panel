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
