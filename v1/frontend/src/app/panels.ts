/**
 * The panel registry: one entry per feature the shell can open.
 *
 * A panel entry names the custom element, the client it needs and the text it
 * shows, and the theme it belongs to. The shell never imports a component
 * directly, so a feature can be added without touching the shell, and the
 * sidebar and tab strip are both derived from this list.
 */

import type { AccountClient } from '../lib/accounts/account-client'
import { HttpAccountClient } from '../lib/accounts/account-client'
import type { FileClient } from '../lib/files/file-client'
import { HttpFileClient } from '../lib/files/file-client'
import type { FirewallClient } from '../lib/firewall/firewall-client'
import { HttpFirewallClient } from '../lib/firewall/firewall-client'
import type { HostInfoClient } from '../lib/hostinfo/host-info-client'
import { HttpHostInfoClient } from '../lib/hostinfo/host-info-client'
import type { NetworkClient } from '../lib/network/network-client'
import { HttpNetworkClient } from '../lib/network/network-client'
import type { ProcessClient } from '../lib/processes/process-client'
import { HttpProcessClient } from '../lib/processes/process-client'
import type { PluginClient } from '../lib/plugins/plugin-client'
import { HttpPluginClient } from '../lib/plugins/plugin-client'
import type { TerminalTransport } from '../lib/terminal/transport'
import { WebSocketTerminalTransport } from '../lib/terminal/websocket-transport'
import type { VolumeClient } from '../lib/volumes/volume-client'
import { HttpVolumeClient } from '../lib/volumes/volume-client'
import { currentApiFetch } from '../lib/http'
import { msg } from '@lit/localize'

import '../components/anth-account-manager'
import '../components/anth-file-manager'
import '../components/anth-firewall'
import '../components/anth-host-info'
import '../components/anth-network-manager'
import '../components/anth-plugin-manager'
import '../components/anth-process-manager'
import '../components/anth-terminal'
import '../components/anth-volume-manager'

/** The stable id of one sidebar group. */
export type ThemeId =
  | 'system'
  | 'storage'
  | 'network'
  | 'access'
  | 'extensions'

export interface ThemeDefinition {
  id: ThemeId
  icon: string
  title: () => string
  /** Compact label for the portrait drawer, where full titles do not fit. */
  shortTitle: () => string
  description: () => string
  /** Slot whose plugin anchors become extra tabs on this theme's page. */
  pluginSlot?: string
}

/**
 * Sidebar groups, in display order.
 *
 * A theme owns the page, its tab strip and the panels listed in
 * `panelsIn(theme.id)`. `Home` is not a theme; the shell renders it separately
 * at `#/`.
 */
export const THEMES: ThemeDefinition[] = [
  {
    id: 'system',
    icon: '🖥️',
    title: () => msg('System'),
    shortTitle: () => msg('System'),
    description: () => msg('Host identity, load average and running processes.'),
  },
  {
    id: 'storage',
    icon: '💾',
    title: () => msg('Storage'),
    shortTitle: () => msg('Data'),
    description: () => msg('Files on disk and block devices with their mounts.'),
  },
  {
    id: 'network',
    icon: '🌐',
    title: () => msg('Network'),
    shortTitle: () => msg('Network'),
    description: () =>
      msg('Interfaces, addresses, traffic and firewall rules.'),
    pluginSlot: 'panel.network.tab',
  },
  {
    id: 'access',
    icon: '👤',
    title: () => msg('Access'),
    shortTitle: () => msg('Access'),
    description: () => msg('Local users and groups, locks and passwords.'),
  },
  {
    id: 'extensions',
    icon: '🧩',
    title: () => msg('Extensions'),
    shortTitle: () => msg('Extras'),
    description: () => msg('Plugin bundles and interactive terminals.'),
  },
]

export interface PanelDefinition {
  /** Stable id used in the route and in the tab strip. */
  id: string
  /** Sidebar group that owns this panel. */
  theme: ThemeId
  /** Custom element tag the shell appends. */
  element: string
  icon: string
  title: () => string
  description: () => string
  /** Builds the client the element expects. Called per mount. */
  createClient?: () => unknown
  /** Property the client is assigned to. Defaults to `client`. */
  clientProperty?: string
  /** Extra properties assigned after the element is created. */
  properties?: Record<string, unknown>
}

/**
 * Panels shown in the shell, grouped by theme and in tab order.
 *
 * The terminal is the exception: it uses a transport rather than a JSON client
 * and opens a WebSocket, so its factory builds the transport.
 */
export const PANELS: PanelDefinition[] = [
  {
    id: 'host-info',
    theme: 'system',
    element: 'anth-host-info',
    icon: '🖥️',
    title: () => msg('Host info'),
    description: () =>
      msg('Operating system, CPU, load average, memory and swap.'),
    createClient: () => new HttpHostInfoClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'processes',
    theme: 'system',
    element: 'anth-process-manager',
    icon: '⚙️',
    title: () => msg('Processes'),
    description: () =>
      msg('Live process table with signals, tree view and spawning.'),
    createClient: () => new HttpProcessClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'files',
    theme: 'storage',
    element: 'anth-file-manager',
    icon: '📁',
    title: () => msg('Files'),
    description: () =>
      msg('Browse the host filesystem, preview, upload and edit.'),
    createClient: () => new HttpFileClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'volumes',
    theme: 'storage',
    element: 'anth-volume-manager',
    icon: '💾',
    title: () => msg('Storage'),
    description: () => msg('Block devices, mounts, mounting and unmounting.'),
    createClient: () => new HttpVolumeClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'network',
    theme: 'network',
    element: 'anth-network-manager',
    icon: '🌐',
    title: () => msg('Network'),
    description: () => msg('Interfaces, addresses, traffic rates and link state.'),
    createClient: () => new HttpNetworkClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'firewall',
    theme: 'network',
    element: 'anth-firewall',
    icon: '🛡️',
    title: () => msg('Firewall'),
    description: () => msg('Managed nftables chain, allow and deny rules.'),
    createClient: () => new HttpFirewallClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'accounts',
    theme: 'access',
    element: 'anth-account-manager',
    icon: '👤',
    title: () => msg('Accounts'),
    description: () => msg('Local users and groups, locks and passwords.'),
    createClient: () => new HttpAccountClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'plugins',
    theme: 'extensions',
    element: 'anth-plugin-manager',
    icon: '🧩',
    title: () => msg('Plugins'),
    description: () =>
      msg('Installed bundles, frozen capabilities and browser installation.'),
    createClient: () => new HttpPluginClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'terminal',
    theme: 'extensions',
    element: 'anth-terminal',
    icon: '⌨️',
    title: () => msg('Terminal'),
    description: () => msg('Interactive shell on the host.'),
    createClient: () =>
      new WebSocketTerminalTransport({
        url: () => terminalUrl(),
      }),
    clientProperty: 'transport',
  },
]

/** The terminal route carries the fitted size in the query string. */
function terminalUrl(): string {
  const params = new URLSearchParams({
    cols: '120',
    rows: '40',
  })
  return `/api/terminal?${params.toString()}`
}

export function findPanel(id: string): PanelDefinition | undefined {
  return PANELS.find((panel) => panel.id === id)
}

export function findTheme(id: ThemeId): ThemeDefinition | undefined {
  return THEMES.find((theme) => theme.id === id)
}

/** Panels owned by a theme, in registry (tab) order. */
export function panelsIn(id: ThemeId): PanelDefinition[] {
  return PANELS.filter((panel) => panel.theme === id)
}

/** The theme that owns a panel, or `undefined` for an unknown id. */
export function themeForPanel(id: string): ThemeDefinition | undefined {
  const panel = findPanel(id)
  return panel ? findTheme(panel.theme) : undefined
}

export type { AccountClient, FileClient, FirewallClient, HostInfoClient }
export type { NetworkClient, PluginClient, ProcessClient, TerminalTransport }
export type { VolumeClient }
