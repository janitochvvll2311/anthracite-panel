import { afterEach, expect, test, vi } from 'vitest'

import './firewall'
import { HttpFirewallClient } from 'anth-client'
import type {
  FirewallClient,
  FirewallRule,
  FirewallSnapshot,
} from 'anth-wire'

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
    rules: [
      rule(),
      rule({
        handle: 5,
        action: 'custom',
        protocol: undefined,
        port: undefined,
        comment: 'manual',
      }),
      rule({
        handle: 6,
        chain: 'output',
        action: 'allow',
        protocol: 'tcp',
        port: 443,
        comment: 'https',
      }),
    ],
    chains: [
      { chain: 'input', initialized: true, policy: 'deny' },
      { chain: 'output', initialized: true, policy: 'allow' },
    ],
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
  const add = vi.fn().mockResolvedValue('denied tcp/80 on input')
  const update = vi.fn().mockResolvedValue('updated firewall rule 3')
  const remove = vi.fn().mockResolvedValue('deleted firewall rule 3')
  const flush = vi.fn().mockResolvedValue('flushed 2 firewall rules from input')
  const setPolicy = vi.fn().mockResolvedValue('set input policy to allow')
  const client: FirewallClient = { get, add, update, remove, flush, setPolicy }
  return { client, get, add, update, remove, flush, setPolicy }
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

test('separates input and output rules into tabs', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const text = element.shadowRoot?.textContent ?? ''

  expect(text).toContain('Managed table')
  expect(text).toContain('inet anthracite')
  expect(text).toContain('input + output')
  expect(text).toContain('2 rules')
  expect(text).toContain('#3')
  expect(text).toContain('tcp/80')
  expect(text).toContain('http')
  expect(text).toContain('#5')
  expect(text).toContain('custom')
  expect(text).toContain('Raw ruleset')
  expect(element.shadowRoot?.querySelectorAll('.rule')).toHaveLength(2)
  expect(element.shadowRoot?.querySelectorAll('.subtab')).toHaveLength(2)
  expect(element.shadowRoot?.querySelectorAll('.chain-row')).toHaveLength(1)
  expect(text).not.toContain('#6')

  const inputTab = element.shadowRoot?.querySelector<HTMLElement>(
    '.subtab[data-chain="input"]',
  )!
  const outputTab = element.shadowRoot?.querySelector<HTMLElement>(
    '.subtab[data-chain="output"]',
  )!
  expect(inputTab.getAttribute('aria-selected')).toBe('true')
  expect(inputTab.textContent).toContain('2')
  expect(outputTab.getAttribute('aria-selected')).toBe('false')
  expect(outputTab.textContent).toContain('1')

  const input = element.shadowRoot?.querySelector<HTMLElement>(
    '.chain-row[data-chain="input"]',
  )
  expect(input?.querySelector<HTMLSelectElement>('select')?.value).toBe('deny')
  expect(button(input!, 'Flush')).not.toBeNull()
  expect(element.shadowRoot?.querySelector('form.add')).not.toBeNull()

  outputTab.click()
  await settle(element)
  expect(outputTab.getAttribute('aria-selected')).toBe('true')
  expect(element.shadowRoot?.querySelectorAll('.rule')).toHaveLength(1)
  expect(element.shadowRoot?.textContent).toContain('#6')
  expect(element.shadowRoot?.textContent).toContain('tcp/443')
  expect(
    element.shadowRoot?.querySelector('.chain-row[data-chain="output"]'),
  ).not.toBeNull()
  expect(
    element.shadowRoot?.querySelector('.chain-row[data-chain="input"]'),
  ).toBeNull()
})

test('adds a rule in the active direction tab', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const outputTab = element.shadowRoot?.querySelector<HTMLElement>(
    '.subtab[data-chain="output"]',
  )!
  outputTab.click()
  await settle(element)

  const form = element.shadowRoot?.querySelector<HTMLFormElement>('form.add')!
  const selects = form.querySelectorAll<HTMLSelectElement>('select')
  selects[0].value = 'allow'
  selects[0].dispatchEvent(new Event('change'))
  const port = form.querySelector<HTMLInputElement>('input[type="number"]')!
  port.value = '53'
  port.dispatchEvent(new Event('input'))
  selects[1].value = 'udp'
  selects[1].dispatchEvent(new Event('change'))
  const comment = form.querySelector<HTMLInputElement>('input:not([type="number"])')!
  comment.value = 'dns'
  comment.dispatchEvent(new Event('input'))
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.add).toHaveBeenCalledWith('output', 'allow', 'udp', 53, 'dns')
  expect(feed.get).toHaveBeenCalledTimes(2)
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
})

test('edits an existing rule, including its chain and action', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const first = element.shadowRoot?.querySelector<HTMLElement>('.rule')!
  button(first, 'Edit').click()
  await element.updateComplete

  const form = first.querySelector<HTMLFormElement>('form.edit')!
  const selects = form.querySelectorAll<HTMLSelectElement>('select')
  selects[0].value = 'output'
  selects[0].dispatchEvent(new Event('change'))
  selects[1].value = 'allow'
  selects[1].dispatchEvent(new Event('change'))
  const port = form.querySelector<HTMLInputElement>('input[type="number"]')!
  port.value = '8443'
  port.dispatchEvent(new Event('input'))
  const comment = form.querySelector<HTMLInputElement>('input:not([type="number"])')!
  comment.value = 'admin'
  comment.dispatchEvent(new Event('input'))
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.update).toHaveBeenCalledWith(
    3,
    'output',
    'allow',
    'tcp',
    8443,
    'admin',
  )
  expect(element.shadowRoot?.querySelector('form.edit')).toBeNull()
  expect(
    element.shadowRoot
      ?.querySelector('.subtab[data-chain="output"]')
      ?.getAttribute('aria-selected'),
  ).toBe('true')
})

test('a 405 from an outdated daemon points at rebuilding it', async () => {
  const feed = fakeClient()
  feed.update.mockRejectedValue(
    Object.assign(new Error('405 Method Not Allowed'), { status: 405 }),
  )
  const element = await mount(feed.client)
  const first = element.shadowRoot?.querySelector<HTMLElement>('.rule')!
  button(first, 'Edit').click()
  await element.updateComplete
  first
    .querySelector<HTMLFormElement>('form.edit')!
    .dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(element.shadowRoot?.textContent).toContain(
    'The daemon does not support this firewall operation. Rebuild and restart it, then reload the page.',
  )
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
  expect(element.shadowRoot?.textContent).toContain(
    'The port must be between 1 and 65535.',
  )
})

test('deletes one rule and flushes one chain after confirmation', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const first = element.shadowRoot?.querySelector<HTMLElement>('.rule')!
  button(first, 'Delete').click()
  await element.updateComplete
  expect(first.querySelector('.confirm')?.textContent).toContain('Delete rule #3?')
  button(first.querySelector('.confirm')!, 'Delete').click()
  await settle(element)
  expect(feed.remove).toHaveBeenCalledWith(3)

  const input = element.shadowRoot?.querySelector<HTMLElement>(
    '.chain-row[data-chain="input"]',
  )!
  button(input, 'Flush').click()
  await element.updateComplete
  const confirm = element.shadowRoot?.querySelector<HTMLElement>('.body > .confirm')
  expect(confirm?.textContent).toContain('Flush every rule from the input chain?')
  button(confirm!, 'Flush').click()
  await settle(element)
  expect(feed.flush).toHaveBeenCalledWith('input')
})

test('changes a chain policy after confirmation and warns about deny', async () => {
  const feed = fakeClient()
  const element = await mount(feed.client)
  const input = element.shadowRoot?.querySelector<HTMLElement>(
    '.chain-row[data-chain="input"]',
  )!
  const select = input.querySelector<HTMLSelectElement>('select')!
  select.value = 'allow'
  select.dispatchEvent(new Event('change'))
  await element.updateComplete
  button(input, 'Apply').click()
  await element.updateComplete

  const confirm = element.shadowRoot?.querySelector<HTMLElement>('.body > .confirm')
  expect(confirm?.textContent).toContain('Set the input policy to allow?')
  button(confirm!, 'Apply').click()
  await settle(element)
  expect(feed.setPolicy).toHaveBeenCalledWith('input', 'allow')

  element.shadowRoot
    ?.querySelector<HTMLElement>('.subtab[data-chain="output"]')
    ?.click()
  await settle(element)
  const output = element.shadowRoot?.querySelector<HTMLElement>(
    '.chain-row[data-chain="output"]',
  )!
  const outputSelect = output.querySelector<HTMLSelectElement>('select')!
  outputSelect.value = 'deny'
  outputSelect.dispatchEvent(new Event('change'))
  await element.updateComplete
  button(output, 'Apply').click()
  await element.updateComplete
  expect(
    element.shadowRoot?.querySelector<HTMLElement>('.body > .confirm')
      ?.textContent,
  ).toContain('lock you out')
})

test('control off hides the add, edit, delete, flush and policy controls', async () => {
  const feed = fakeClient(snapshot({ control_enabled: false }))
  const element = await mount(feed.client)
  expect(element.shadowRoot?.querySelector('form.add')).toBeNull()
  expect(element.shadowRoot?.querySelector('.edit-rule')).toBeNull()
  expect(element.shadowRoot?.querySelector('.delete')).toBeNull()
  expect(element.shadowRoot?.querySelector('.chain-row select')).toBeNull()
  expect(element.shadowRoot?.textContent).not.toContain('Flush')
  expect(element.shadowRoot?.textContent).toContain('read-only')
  expect(element.shadowRoot?.querySelectorAll('.rule')).toHaveLength(2)
})

test('an uninitialized table is a normal empty state', async () => {
  const feed = fakeClient(
    snapshot({
      initialized: false,
      rules: [],
      chains: [
        { chain: 'input', initialized: false, policy: 'allow' },
        { chain: 'output', initialized: false, policy: 'allow' },
      ],
      ruleset: '',
    }),
  )
  const element = await mount(feed.client)
  const text = element.shadowRoot?.textContent ?? ''
  expect(text).toContain('The managed table does not exist yet.')
  expect(text).toContain('No rules')
  expect(text).toContain('not created')
  expect(element.shadowRoot?.querySelector('form.add')).not.toBeNull()
  const input = element.shadowRoot?.querySelector<HTMLElement>(
    '.chain-row[data-chain="input"]',
  )!
  expect(button(input, 'Flush').disabled).toBe(true)
})

test('an old daemon without chain states is called out', async () => {
  const feed = fakeClient(snapshot({ chains: [] }))
  const element = await mount(feed.client)
  expect(element.shadowRoot?.textContent).toContain(
    'This daemon does not report the input and output chains. Rebuild and restart it, then reload the page.',
  )
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

  await expect(client.add('input', 'deny', 'udp', 53, 'dns')).resolves.toEqual({ message: 'ok' })
  await expect(client.update(4, 'output', 'allow', 'tcp', 443, 'https')).resolves.toEqual({ message: 'ok' })
  await expect(client.remove(4)).resolves.toEqual({ message: 'ok' })
  await expect(client.flush('output')).resolves.toEqual({ message: 'ok' })
  await expect(client.setPolicy('input', 'deny')).resolves.toEqual({ message: 'ok' })

  expect(calls.map((call) => call.url)).toEqual([
    '/api/firewall/rule',
    '/api/firewall/update',
    '/api/firewall/delete',
    '/api/firewall/flush',
    '/api/firewall/policy',
  ])
  expect(calls.map((call) => JSON.parse(String(call.init?.body)))).toEqual([
    { chain: 'input', action: 'deny', protocol: 'udp', port: 53, comment: 'dns' },
    {
      handle: 4,
      chain: 'output',
      action: 'allow',
      protocol: 'tcp',
      port: 443,
      comment: 'https',
    },
    { handle: 4 },
    { chain: 'output' },
    { chain: 'input', policy: 'deny' },
  ])
})
