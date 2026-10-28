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
