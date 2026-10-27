import { afterEach, expect, test, vi } from 'vitest'

import './firewall'
import { HttpFirewallClient } from './firewall-client'
import type {
  FirewallClient,
  FirewallRule,
  FirewallSnapshot,
} from './firewall-client'

function rule(overrides: Partial<FirewallRule> = {}): FirewallRule {
  return {
    handle: 3,
    chain: 'input',
    action: 'deny',
    protocol: 'tcp',
    port: 80,
    comment: 'http',
    ...overrides,
  }
}

function snapshot(overrides: Partial<FirewallSnapshot> = {}): FirewallSnapshot {
  return {
    initialized: true,
    control_enabled: true,
    rules: [rule(), rule({ handle: 5, action: 'custom', protocol: undefined, port: undefined, comment: 'manual' })],
    ruleset: 'table inet anthracite { chain input { ... } }',
    timestamp_ms: 1,
    ...overrides,
  }
}

type FirewallElement = HTMLElement & {
  client: FirewallClient
  updateComplete: Promise<boolean>
}

function fakeClient(initial = snapshot()) {
  const get = vi.fn().mockResolvedValue(initial)
  const add = vi.fn().mockResolvedValue('denied tcp/80')
  const remove = vi.fn().mockResolvedValue('deleted firewall rule 3')
  const flush = vi.fn().mockResolvedValue('flushed 2 firewall rules')
  const client: FirewallClient = { get, add, remove, flush }
  return { client, get, add, remove, flush }
}

async function settle(element: FirewallElement): Promise<void> {
  for (let index = 0; index < 6; index += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(client: FirewallClient): Promise<FirewallElement> {
  const element = document.createElement('anth-firewall') as FirewallElement
  element.client = client
  document.body.append(element)
  await settle(element)
  return element
}

function button(container: ParentNode, label: string): HTMLButtonElement {
  const found = [...container.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!found) throw new Error(`missing button: ${label}`)
  return found
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

test('renders managed and custom rules plus the raw ruleset', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const text = element.shadowRoot?.textContent ?? ''

  expect(text).toContain('Managed table')
  expect(text).toContain('inet anthracite')
  expect(text).toContain('2 rules')
  expect(text).toContain('#3')
  expect(text).toContain('tcp/80')
  expect(text).toContain('http')
  expect(text).toContain('#5')
  expect(text).toContain('custom')
  expect(text).toContain('Raw ruleset')
  expect(element.shadowRoot?.querySelectorAll('.rule')).toHaveLength(2)
  expect(element.shadowRoot?.querySelector('form.add')).not.toBeNull()
})

test('adds a rule with the selected action, protocol, port and comment', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const form = element.shadowRoot?.querySelector<HTMLFormElement>('form.add')!

  const action = form.querySelector<HTMLSelectElement>('select')!
  action.value = 'allow'
  action.dispatchEvent(new Event('change'))
  const port = form.querySelector<HTMLInputElement>('input[type="number"]')!
  port.value = '53'
  port.dispatchEvent(new Event('input'))
  const selects = form.querySelectorAll<HTMLSelectElement>('select')
  selects[1].value = 'udp'
  selects[1].dispatchEvent(new Event('change'))
  const comment = form.querySelector<HTMLInputElement>('input:not([type="number"])')!
  comment.value = 'dns'
  comment.dispatchEvent(new Event('input'))
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.add).toHaveBeenCalledWith('allow', 'udp', 53, 'dns')
  expect(feed.get).toHaveBeenCalledTimes(2)
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
})

test('rejects an out-of-range port before calling the client', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const form = element.shadowRoot?.querySelector<HTMLFormElement>('form.add')!
  const port = form.querySelector<HTMLInputElement>('input[type="number"]')!
  port.value = '0'
  port.dispatchEvent(new Event('input'))
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.add).not.toHaveBeenCalled()
  expect(element.shadowRoot?.textContent).toContain('The port must be between 1 and 65535.')
})

test('deletes one rule and flushes the chain after confirmation', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const first = element.shadowRoot?.querySelector<HTMLElement>('.rule')!
  button(first, 'Delete').click()
  await element.updateComplete
  expect(first.querySelector('.confirm')?.textContent).toContain('Delete rule #3?')
  button(first.querySelector('.confirm')!, 'Delete').click()
  await settle(element)
  expect(feed.remove).toHaveBeenCalledWith(3)

  button(element.shadowRoot!, 'Flush').click()
  await element.updateComplete
  const confirm = element.shadowRoot?.querySelector<HTMLElement>('.toolbar + .confirm')
  expect(confirm?.textContent).toContain('Flush every rule from the managed chain?')
  button(confirm!, 'Flush').click()
  await settle(element)
  expect(feed.flush).toHaveBeenCalled()
})

test('control off hides the add, delete and flush controls', async () => {
  const feed = fakeClient(snapshot({ control_enabled: false }))
  const element = await mount(feed.client)
  expect(element.shadowRoot?.querySelector('form.add')).toBeNull()
  expect(element.shadowRoot?.querySelector('.delete')).toBeNull()
  expect(element.shadowRoot?.textContent).not.toContain('Flush')
  expect(element.shadowRoot?.textContent).toContain('read-only')
  expect(element.shadowRoot?.querySelectorAll('.rule')).toHaveLength(2)
})

test('an uninitialized table is a normal empty state', async () => {
  const feed = fakeClient(
    snapshot({ initialized: false, rules: [], ruleset: '' }),
  )
  const element = await mount(feed.client)
  const text = element.shadowRoot?.textContent ?? ''
  expect(text).toContain('The managed table does not exist yet.')
  expect(text).toContain('No rules')
  expect(element.shadowRoot?.querySelector('form.add')).not.toBeNull()
  expect(button(element.shadowRoot!, 'Flush').disabled).toBe(true)
})

test('a failed read keeps the message visible', async () => {
  const feed = fakeClient()
  feed.get.mockRejectedValue(new Error('nft is not available'))
  const element = await mount(feed.client)
  expect(element.shadowRoot?.textContent).toContain('nft is not available')
})

test('the HTTP client reads the snapshot and surfaces daemon errors', async () => {
  const calls: string[] = []
  const fakeFetch = vi.fn((url: string) => {
    calls.push(url)
    return Promise.resolve(new Response(JSON.stringify(snapshot())))
  }) as unknown as typeof fetch
  const client = new HttpFirewallClient(fakeFetch)
  await expect(client.get()).resolves.toMatchObject({ initialized: true })
  expect(calls).toEqual(['/api/firewall'])

  const failing = new HttpFirewallClient((() =>
    Promise.resolve(
      new Response(
        JSON.stringify({
          error: { kind: 'forbidden', code: 'nft_permission', message: 'no permission' },
        }),
        { status: 403 },
      ),
    )) as unknown as typeof fetch)
  await expect(failing.get()).rejects.toThrow('no permission')
})

test('the HTTP client sends the firewall bodies', async () => {
  const calls: Array<{ url: string; init?: RequestInit }> = []
  const fakeFetch = vi.fn((url: string, init?: RequestInit) => {
    calls.push({ url, init })
    return Promise.resolve(new Response(JSON.stringify({ message: 'ok' })))
  }) as unknown as typeof fetch
  const client = new HttpFirewallClient(fakeFetch)

  await expect(client.add('deny', 'udp', 53, 'dns')).resolves.toBe('ok')
  await expect(client.remove(4)).resolves.toBe('ok')
  await expect(client.flush()).resolves.toBe('ok')

  expect(calls.map((call) => call.url)).toEqual([
    '/api/firewall/rule',
    '/api/firewall/delete',
    '/api/firewall/flush',
  ])
  expect(calls.map((call) => JSON.parse(String(call.init?.body)))).toEqual([
    { action: 'deny', protocol: 'udp', port: 53, comment: 'dns' },
    { handle: 4 },
    {},
  ])
})
