import { afterEach, describe, expect, it } from 'vitest'

import './anth-account-manager'
import './anth-file-manager'
import './anth-firewall'
import './anth-host-info'
import './anth-network-manager'
import './anth-plugin-manager'
import './anth-process-manager'
import './anth-volume-manager'

import { accountSnapshot, FakeAccountClient } from '../test/fake-account-client'
import {
  FakeFileClient,
  fileListing,
} from '../test/fake-file-client'
import {
  FakeFirewallClient,
  firewallSnapshot,
} from '../test/fake-firewall-client'
import { FakeHostInfoClient } from '../test/fake-host-info-client'
import {
  FakeNetworkClient,
  networkSnapshot,
} from '../test/fake-network-client'
import {
  FakePluginClient,
  pluginsCatalogue,
} from '../test/fake-plugin-client'
import {
  FakeProcessClient,
  processSnapshot,
} from '../test/fake-process-client'
import { FakeVolumeClient } from '../test/fake-volume-client'

interface LitHost extends HTMLElement {
  updateComplete: Promise<boolean>
}

async function settle(element: LitHost): Promise<void> {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount<T extends LitHost>(
  tag: string,
  client: unknown,
): Promise<T> {
  const element = document.createElement(tag) as T
  ;(element as unknown as Record<string, unknown>).client = client
  document.body.append(element)
  await element.updateComplete
  await settle(element)
  return element
}

function fab(element: LitHost): HTMLElement | null {
  return element.shadowRoot?.querySelector('anth-fab') ?? null
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('panel FABs', () => {
  it('offers a refresh FAB on the host and storage panels', async () => {
    const host = await mount<LitHost>('anth-host-info', new FakeHostInfoClient())
    expect(fab(host)?.getAttribute('label')).toBe('Refresh now')

    const volumes = await mount<LitHost>(
      'anth-volume-manager',
      new FakeVolumeClient(),
    )
    expect(fab(volumes)?.getAttribute('label')).toBe('Refresh')
  })

  it('only offers the process spawn FAB when spawning is enabled', async () => {
    const enabled = new FakeProcessClient()
    enabled.current = processSnapshot({
      spawn_enabled: true,
      spawn_allow: ['/bin/echo'],
    })
    const withSpawn = await mount<LitHost>('anth-process-manager', enabled)
    expect(fab(withSpawn)?.getAttribute('label')).toBe('Start process')

    const disabled = new FakeProcessClient()
    const withoutSpawn = await mount<LitHost>('anth-process-manager', disabled)
    expect(fab(withoutSpawn)).toBeNull()
  })

  it('only offers the upload FAB when the listing is writable', async () => {
    const writable = new FakeFileClient()
    writable.listings.set('/', fileListing({ write_enabled: true }))
    const withWrite = await mount<LitHost>('anth-file-manager', writable)
    expect(fab(withWrite)?.getAttribute('label')).toBe('Upload')

    const readOnly = new FakeFileClient()
    readOnly.listings.set('/', fileListing({ write_enabled: false }))
    const withoutWrite = await mount<LitHost>('anth-file-manager', readOnly)
    expect(fab(withoutWrite)).toBeNull()
  })

  it('only offers interface creation when management is enabled', async () => {
    const enabled = new FakeNetworkClient()
    enabled.current = networkSnapshot([], { manage_enabled: true })
    const withManage = await mount<LitHost>('anth-network-manager', enabled)
    expect(fab(withManage)?.getAttribute('label')).toBe('Create interface')

    const disabled = new FakeNetworkClient()
    disabled.current = networkSnapshot([], { manage_enabled: false })
    const withoutManage = await mount<LitHost>('anth-network-manager', disabled)
    expect(fab(withoutManage)).toBeNull()
  })

  it('only offers firewall, account and plugin actions when control allows it', async () => {
    const firewall = new FakeFirewallClient()
    firewall.current = firewallSnapshot({ control_enabled: true })
    const withFirewall = await mount<LitHost>('anth-firewall', firewall)
    expect(fab(withFirewall)?.getAttribute('label')).toBe('Add rule')

    const accounts = new FakeAccountClient(
      accountSnapshot({ control_enabled: true }),
    )
    const withAccounts = await mount<LitHost>('anth-account-manager', accounts)
    expect(fab(withAccounts)?.getAttribute('label')).toBe('New user')

    const plugins = new FakePluginClient()
    plugins.current = pluginsCatalogue({ install_from_web: true, enabled: true })
    const withPlugins = await mount<LitHost>('anth-plugin-manager', plugins)
    expect(fab(withPlugins)?.getAttribute('label')).toBe('Install plugin')

    const readOnly = new FakeFirewallClient()
    readOnly.current = firewallSnapshot({ control_enabled: false })
    const withoutFirewall = await mount<LitHost>('anth-firewall', readOnly)
    expect(fab(withoutFirewall)).toBeNull()
  })
})
