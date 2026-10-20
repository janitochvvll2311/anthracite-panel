import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-firewall'
import type { AnthFirewall } from './anth-firewall'
import {
  FakeFirewallClient,
  firewallSnapshot,
} from '../test/fake-firewall-client'

async function flush(element: AnthFirewall): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  client: FakeFirewallClient,
  options: { autoLoad?: boolean } = {},
): Promise<AnthFirewall> {
  const element = document.createElement('anth-firewall') as AnthFirewall
  element.client = client
  if (options.autoLoad !== undefined) element.autoLoad = options.autoLoad
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

function button(element: AnthFirewall, label: string): HTMLButtonElement {
  const match = [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

function findButton(
  element: AnthFirewall,
  label: string,
): HTMLButtonElement | undefined {
  return [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
}

function input(element: AnthFirewall, selector: string): HTMLInputElement {
  const match = element.shadowRoot?.querySelector<HTMLInputElement>(selector)
  if (!match) throw new Error(`input ${selector} was not rendered`)
  return match
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('AnthFirewall', () => {
  it('loads the snapshot and renders the managed rules', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client)
    const root = element.shadowRoot

    expect(client.get).toHaveBeenCalledTimes(1)
    expect(root?.textContent).toContain('managed table ready')
    expect(root?.textContent).toContain('1 rule')
    expect(root?.textContent).toContain('3')
    expect(root?.textContent).toContain('ALLOW')
    expect(root?.textContent).toContain('tcp/22')
    expect(root?.textContent).toContain('ssh')
    expect(root?.textContent).toContain('table inet anthracite')
    expect(root?.querySelector('form')).not.toBeNull()
  })

  it('reports a shortened snapshot from the daemon', async () => {
    const client = new FakeFirewallClient()
    client.emit(
      firewallSnapshot({
        total: 6000,
        truncated: true,
        ruleset: '',
        warning: 'showing 1 of 6000 firewall rules, lowest handle first',
      }),
    )
    const element = await mount(client)
    const root = element.shadowRoot

    expect(root?.textContent).toContain('1 of 6000 rules')
    expect(root?.querySelector('.warning')?.textContent?.trim()).toBe(
      'showing 1 of 6000 firewall rules, lowest handle first',
    )
    expect(root?.querySelector('.ruleset')).toBeNull()
  })

  it('adds a rule, clears the form and refreshes the snapshot', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client)

    input(element, 'input[type="number"]').value = '8080'
    input(element, 'input[type="number"]').dispatchEvent(
      new Event('input', { bubbles: true }),
    )
    input(element, 'input[type="text"]').value = 'web'
    input(element, 'input[type="text"]').dispatchEvent(
      new Event('input', { bubbles: true }),
    )

    const form = element.shadowRoot?.querySelector('form')
    if (!form) throw new Error('form was not rendered')
    form.dispatchEvent(
      new Event('submit', { bubbles: true, cancelable: true }),
    )
    await flush(element)

    expect(client.addCalls).toEqual([
      { action: 'allow', port: 8080, protocol: 'tcp', comment: 'web' },
    ])
    expect(client.get).toHaveBeenCalledTimes(2)
    expect(element.shadowRoot?.querySelector('.notice')?.textContent).toContain(
      'allowed tcp/8080',
    )
    expect(input(element, 'input[type="number"]').value).toBe('')
  })

  it('does not refresh while a mutation is in flight', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client)
    let finish!: (message: string) => void
    client.addRule = vi.fn(
      () =>
        new Promise<string>((resolve) => {
          finish = resolve
        }),
    )

    input(element, 'input[type="number"]').value = '8080'
    input(element, 'input[type="number"]').dispatchEvent(
      new Event('input', { bubbles: true }),
    )
    const form = element.shadowRoot?.querySelector('form')
    if (!form) throw new Error('form was not rendered')
    form.dispatchEvent(
      new Event('submit', { bubbles: true, cancelable: true }),
    )
    await flush(element)

    expect(client.get).toHaveBeenCalledTimes(1)
    await element.refresh()
    expect(client.get).toHaveBeenCalledTimes(1)

    finish('allowed tcp/8080')
    await flush(element)

    expect(client.get).toHaveBeenCalledTimes(2)
  })

  it('validates the port and comment before calling the client', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client)

    const form = element.shadowRoot?.querySelector('form')
    if (!form) throw new Error('form was not rendered')
    form.dispatchEvent(
      new Event('submit', { bubbles: true, cancelable: true }),
    )
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'Port must be',
    )

    input(element, 'input[type="number"]').value = '22'
    input(element, 'input[type="number"]').dispatchEvent(
      new Event('input', { bubbles: true }),
    )
    input(element, 'input[type="text"]').value = 'not valid'
    input(element, 'input[type="text"]').dispatchEvent(
      new Event('input', { bubbles: true }),
    )
    form.dispatchEvent(
      new Event('submit', { bubbles: true, cancelable: true }),
    )
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'Comment must be',
    )
    expect(client.addRule).not.toHaveBeenCalled()
  })

  it('deletes a rule after confirmation', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client)
    const confirm = vi.fn(() => true)
    vi.stubGlobal('confirm', confirm)

    button(element, 'Delete').click()
    await flush(element)

    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('handle 3'))
    expect(client.deleteCalls).toEqual([3])
    expect(client.get).toHaveBeenCalledTimes(2)
  })

  it('does not delete when confirmation is cancelled', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client)
    vi.stubGlobal('confirm', vi.fn(() => false))

    button(element, 'Delete').click()
    await flush(element)

    expect(client.deleteRule).not.toHaveBeenCalled()
  })

  it('flushes the managed chain after confirmation', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client)
    const confirm = vi.fn(() => true)
    vi.stubGlobal('confirm', confirm)

    button(element, 'Flush rules').click()
    await flush(element)

    expect(confirm).toHaveBeenCalled()
    expect(client.flushCalls).toBe(1)
    expect(client.get).toHaveBeenCalledTimes(2)
  })

  it('hides mutating controls when the daemon disables them', async () => {
    const client = new FakeFirewallClient()
    client.current = firewallSnapshot({ control_enabled: false })
    const element = await mount(client)

    expect(element.shadowRoot?.querySelector('form')).toBeNull()
    expect(findButton(element, 'Delete')).toBeUndefined()
    expect(findButton(element, 'Flush rules')).toBeUndefined()
    expect(element.shadowRoot?.textContent).toContain(
      'Firewall control is disabled by the daemon.',
    )
  })

  it('does not auto-load when autoLoad is false', async () => {
    const client = new FakeFirewallClient()
    const element = await mount(client, { autoLoad: false })

    expect(client.get).not.toHaveBeenCalled()
    button(element, 'Refresh').click()
    await flush(element)
    expect(client.get).toHaveBeenCalledTimes(1)
  })

  it('surfaces fetch and mutation errors', async () => {
    const client = new FakeFirewallClient()
    client.getError = new Error('daemon unavailable')
    const element = await mount(client)
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'daemon unavailable',
    )

    client.getError = undefined
    client.addError = new Error('nft is not installed')
    await element.refresh()
    input(element, 'input[type="number"]').value = '22'
    input(element, 'input[type="number"]').dispatchEvent(
      new Event('input', { bubbles: true }),
    )
    const form = element.shadowRoot?.querySelector('form')
    if (!form) throw new Error('form was not rendered')
    form.dispatchEvent(
      new Event('submit', { bubbles: true, cancelable: true }),
    )
    await flush(element)
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'nft is not installed',
    )
  })
})
