import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-network-manager'
import type { AnthNetworkManager } from './anth-network-manager'
import { ANTH_NETWORK_STATE_EVENT } from './anth-network-manager'
import {
  FakeNetworkClient,
  networkInterface,
  networkSnapshot,
} from '../test/fake-network-client'
import { answerDialogs, dialogText, resetDialogs } from '../test/dialogs'
import { ANTH_TAB_SHOW_EVENT } from '../lib/ui/tab-visibility'

async function flush(element: AnthNetworkManager): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  client: FakeNetworkClient,
  options: { autoStart?: boolean; refreshInterval?: number } = {},
): Promise<AnthNetworkManager> {
  const element = document.createElement(
    'anth-network-manager',
  ) as AnthNetworkManager
  element.client = client
  if (options.autoStart !== undefined) element.autoStart = options.autoStart
  if (options.refreshInterval !== undefined) {
    element.refreshInterval = options.refreshInterval
  }
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

function findButton(
  element: AnthNetworkManager,
  label: string,
): HTMLButtonElement | undefined {
  return [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
}

function button(element: AnthNetworkManager, label: string): HTMLButtonElement {
  const match = findButton(element, label)
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

function rows(element: AnthNetworkManager): HTMLTableRowElement[] {
  return [
    ...(element.shadowRoot?.querySelectorAll<HTMLTableRowElement>('tbody tr') ??
      []),
  ]
}

afterEach(() => {
  resetDialogs()
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('AnthNetworkManager', () => {
  it('restarts the stream when a tab panel shows it again', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    expect(client.watchCalls).toHaveLength(1)

    element.remove()
    document.body.append(element)
    element.dispatchEvent(new CustomEvent(ANTH_TAB_SHOW_EVENT))
    await flush(element)

    expect(client.watchCalls).toHaveLength(2)
  })

  it('loads a snapshot and renders interfaces, addresses and totals', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    const root = element.shadowRoot

    expect(client.watchCalls).toEqual([2000])
    expect(client.list).toHaveBeenCalledTimes(1)
    expect(root?.textContent).toContain('eth0')
    expect(root?.textContent).toContain('192.0.2.10/24')
    expect(root?.textContent).toContain('2.0 KiB')
    expect(root?.textContent).toContain('1.0 KiB')
    expect(root?.querySelectorAll('tbody tr')).toHaveLength(1)
    expect(root?.querySelector('.status')?.getAttribute('data-status')).toBe(
      'connecting',
    )
  })

  it('shows a truncation warning from the daemon', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([networkInterface()], {
      total: 3,
      truncated: true,
      warning: 'showing 1 of 3 interfaces',
    })
    const element = await mount(client)

    expect(
      element.shadowRoot?.querySelector('.warning')?.textContent,
    ).toContain('showing 1 of 3 interfaces')
  })

  it('computes rates from consecutive snapshots', async () => {
    const client = new FakeNetworkClient()
    const first = networkSnapshot()
    client.current = first
    const element = await mount(client)

    expect(rows(element)[0]?.textContent).toContain('—')

    client.emit(
      networkSnapshot(
        [networkInterface({ rx_bytes: 6144, tx_bytes: 3072 })],
        { timestamp_ms: first.timestamp_ms + 2000 },
      ),
    )
    await element.updateComplete

    expect(rows(element)[0]?.textContent).toContain('2.0 KiB/s')
    expect(rows(element)[0]?.textContent).toContain('1.0 KiB/s')
  })

  it('follows live status transitions and stream errors', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)

    client.status('live')
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.status')?.getAttribute('data-status')).toBe(
      'live',
    )

    client.fail(new Error('stream failed'))
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'stream failed',
    )
  })

  it('filters and sorts the table', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([
      networkInterface(),
      networkInterface({
        name: 'lo',
        index: 1,
        state: 'unknown',
        mac: '00:00:00:00:00:00',
        addresses: [{ family: 'ipv4', address: '127.0.0.1', prefix_len: 8 }],
        rx_bytes: 4096,
      }),
    ])
    const element = await mount(client)

    const filter = element.shadowRoot?.querySelector<HTMLInputElement>(
      'input[type="search"]',
    )
    if (!filter) throw new Error('filter was not rendered')
    filter.value = '127.0.0.1'
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    await element.updateComplete
    expect(rows(element)).toHaveLength(1)
    expect(rows(element)[0]?.textContent).toContain('lo')

    filter.value = ''
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    await element.updateComplete
    ;(
      element.shadowRoot?.querySelector(
        'button[aria-label="Sort by rx"]',
      ) as HTMLButtonElement
    ).click()
    await element.updateComplete

    expect(rows(element)[0]?.textContent).toContain('lo')
  })

  it('selects an interface and shows its details', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)

    rows(element)[0]?.click()
    await element.updateComplete

    expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain(
      'MTU',
    )
    expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain(
      '1500',
    )
    expect(element.shadowRoot?.querySelector('.details')?.textContent).toContain(
      '2001:db8::10/64',
    )
  })

  it('brings an interface down after confirmation', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    const listener = vi.fn()
    element.addEventListener(ANTH_NETWORK_STATE_EVENT, listener)

    button(element, 'Down').click()
    await flush(element)

    expect(dialogText()).toContain('Bring down eth0?')
    await answerDialogs('confirm')
    await flush(element)

    expect(client.stateCalls).toEqual([{ name: 'eth0', up: false }])
    expect(listener).toHaveBeenCalledWith(
      expect.objectContaining({ detail: { name: 'eth0', up: false } }),
    )
  })

  it('does not apply stream snapshots while a state change is in flight', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    let finish!: () => void
    client.setState = vi.fn(
      (_name: string, _up: boolean) =>
        new Promise<void>((resolve) => {
          finish = resolve
        }),
    )

    button(element, 'Down').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    client.emit(
      networkSnapshot([
        networkInterface({ state: 'down', up: false, running: false }),
      ]),
    )
    await element.updateComplete

    expect(findButton(element, 'Up')).toBeUndefined()
    expect(findButton(element, 'Down')).toBeDefined()
    expect(client.list).toHaveBeenCalledTimes(1)

    finish()
    await flush(element)

    expect(findButton(element, 'Up')).toBeDefined()
    expect(client.list).toHaveBeenCalledTimes(2)
  })

  it('does not change an interface when confirmation is cancelled', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    button(element, 'Down').click()
    await flush(element)
    await answerDialogs('cancel')
    await flush(element)

    expect(client.setState).not.toHaveBeenCalled()
  })

  it('hides controls when the daemon disables them', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([networkInterface()], {
      control_enabled: false,
    })
    const element = await mount(client)

    expect(findButton(element, 'Down')).toBeUndefined()
    expect(element.shadowRoot?.textContent).toContain(
      'Control is disabled by the daemon.',
    )
  })

  it('protects loopback from being brought down', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([
      networkInterface({
        name: 'lo',
        index: 1,
        state: 'unknown',
        loopback: true,
      }),
    ])
    const element = await mount(client)

    expect(element.shadowRoot?.textContent).toContain('protected')
    expect(findButton(element, 'Down')).toBeUndefined()
  })

  it('surfaces a state-change error', async () => {
    const client = new FakeNetworkClient()
    client.stateError = new Error('permission denied')
    const element = await mount(client)

    button(element, 'Down').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'permission denied',
    )

    client.emit(networkSnapshot())
    await element.updateComplete
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'permission denied',
    )
  })

  it('pauses, resumes and refreshes without duplicating the stream', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)

    button(element, 'Pause').click()
    await element.updateComplete
    expect(client.unsubscribes).toBe(1)
    expect(element.shadowRoot?.querySelector('.status')?.getAttribute('data-status')).toBe(
      'closed',
    )

    button(element, 'Resume').click()
    await flush(element)
    expect(client.watchCalls).toHaveLength(2)
    expect(client.list).toHaveBeenCalledTimes(2)

    button(element, 'Refresh').click()
    await flush(element)
    expect(client.list).toHaveBeenCalledTimes(3)
    expect(client.watchCalls).toHaveLength(2)
  })

  it('does not start automatically when autoStart is false', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client, { autoStart: false })

    expect(client.watch).not.toHaveBeenCalled()
    expect(client.list).not.toHaveBeenCalled()
    expect(button(element, 'Resume')).toBeTruthy()
  })

  it('reports a failed initial snapshot', async () => {
    const client = new FakeNetworkClient()
    client.listError = new Error('daemon unavailable')
    const element = await mount(client)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'daemon unavailable',
    )
  })
})

describe('management', () => {
  afterEach(() => {
    document.body.innerHTML = ''
    resetDialogs()
  })

  it('hides the Create button when the daemon refuses management', async () => {
    const client = new FakeNetworkClient()
    client.current = networkSnapshot([networkInterface()], {
      manage_enabled: false,
    })
    const element = await mount(client)

    const button = findButton(element, 'Create')
    expect(button?.disabled).toBe(true)
  })

  it('creates an interface from the form', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)

    findButton(element, 'Create')!.click()
    await element.updateComplete

    const form = element.shadowRoot!.querySelector('form') as HTMLFormElement
    ;(form.elements.namedItem('name') as HTMLInputElement).value = 'br0'
    ;(form.elements.namedItem('kind') as HTMLSelectElement).value = 'veth'
    ;(form.elements.namedItem('link') as HTMLInputElement).value = 'veth1'
    ;(form.elements.namedItem('mtu') as HTMLInputElement).value = '9000'
    form.dispatchEvent(new Event('submit', { cancelable: true }))
    await flush(element)

    expect(client.createCalls).toEqual([
      { name: 'br0', kind: 'veth', peer: 'veth1', mtu: 9000, up: false },
    ])
    // The form closes after a successful create.
    expect(element.shadowRoot!.querySelector('form')).toBeNull()
  })

  it('sends a vlan parent in `link`, not in `peer`', async () => {
    // The daemon reads a different field per kind; sending the wrong one is
    // accepted by the type but refused by the daemon.
    const client = new FakeNetworkClient()
    const element = await mount(client)

    findButton(element, 'Create')!.click()
    await element.updateComplete
    const form = element.shadowRoot!.querySelector('form') as HTMLFormElement
    ;(form.elements.namedItem('name') as HTMLInputElement).value = 'eth0.10'
    ;(form.elements.namedItem('kind') as HTMLSelectElement).value = 'vlan'
    ;(form.elements.namedItem('link') as HTMLInputElement).value = 'eth0'
    ;(form.elements.namedItem('vlan_id') as HTMLInputElement).value = '10'
    form.dispatchEvent(new Event('submit', { cancelable: true }))
    await flush(element)

    expect(client.createCalls[0].link).toBe('eth0')
    expect(client.createCalls[0].peer).toBeUndefined()
    expect(client.createCalls[0].vlan_id).toBe(10)
  })

  it('warns that a physical device will lose its link before deleting', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    rows(element)[0]?.click()
    await flush(element)

    findButton(element, 'Delete')!.click()
    // Read the prompt before answering it: the host clears the dialog on
    // settle, so `dialogText()` afterwards is empty by design.
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(dialogText()).toContain('physical device')

    await answerDialogs('confirm')
    await flush(element)

    expect(client.removeCalls).toEqual(['eth0'])
  })

  it('disables Delete for a loopback and never calls the client', async () => {
    const client = new FakeNetworkClient()
    client.emit(
      networkSnapshot([
        networkInterface({
          name: 'lo',
          kind: 'loopback',
          loopback: true,
          physical: false,
        }),
      ]),
    )
    const element = await mount(client)
    rows(element)[0]?.click()
    await flush(element)

    const button = findButton(element, 'Delete')
    expect(button?.disabled).toBe(true)
    expect(button?.title).toContain('loopback')
    expect(client.removeCalls).toEqual([])
  })

  it('refuses to rename an interface that is up', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    rows(element)[0]?.click()
    await flush(element)

    const button = findButton(element, 'Rename')
    expect(button?.disabled).toBe(true)
    expect(button?.title).toContain('down first')
  })

  it('renames through the dialog once the link is down', async () => {
    const client = new FakeNetworkClient()
    client.emit(
      networkSnapshot([networkInterface({ name: 'd0', kind: 'dummy', up: false })]),
    )
    const element = await mount(client)
    rows(element)[0]?.click()
    await flush(element)

    findButton(element, 'Rename')!.click()
    await answerDialogs('confirm', 'd1')
    await flush(element)

    expect(client.renameCalls).toEqual([{ name: 'd0', newName: 'd1' }])
  })

  it('sends only the MTU on an edit, so the link state is untouched', async () => {
    const client = new FakeNetworkClient()
    const element = await mount(client)
    rows(element)[0]?.click()
    await flush(element)

    findButton(element, 'Edit MTU')!.click()
    await answerDialogs('confirm', '9000')
    await flush(element)

    expect(client.editCalls).toEqual([{ name: 'eth0', request: { mtu: 9000 } }])
  })

  it('shows the daemon error and keeps the form open when a create fails', async () => {
    const client = new FakeNetworkClient()
    client.mutateError = new Error('interface `br0` already exists')
    const element = await mount(client)

    findButton(element, 'Create')!.click()
    await element.updateComplete
    const form = element.shadowRoot!.querySelector('form') as HTMLFormElement
    ;(form.elements.namedItem('name') as HTMLInputElement).value = 'br0'
    form.dispatchEvent(new Event('submit', { cancelable: true }))
    await flush(element)

    const alert = element.shadowRoot!.querySelector('[role="alert"]')
    expect(alert?.textContent).toContain('already exists')
    expect(element.shadowRoot!.querySelector('form')).not.toBeNull()
  })
})
