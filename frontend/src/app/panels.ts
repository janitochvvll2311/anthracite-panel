/**
 * The panel registry: one entry per feature the shell can open.
 *
 * A panel entry names the custom element, the client it needs and the text it
 * shows. The shell never imports a component directly, so a feature can be
 * added without touching the shell, and a later window manager can render the
 * same entry inside a floating window.
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
import '../components/anth-network-monitor'
import '../components/anth-process-manager'
import '../components/anth-terminal'
import '../components/anth-volume-manager'

export interface PanelDefinition {
  /** Stable id used in the route and, later, in a window title. */
  id: string
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
 * Panels shown on the home page, in order.
 *
 * The terminal is the exception: it uses a transport rather than a JSON client
 * and opens a WebSocket, so its factory builds the transport.
 */
export const PANELS: PanelDefinition[] = [
  {
    id: 'host-info',
    element: 'anth-host-info',
    icon: '🖥️',
    title: () => msg('Host info'),
    description: () =>
      msg('Operating system, CPU, load average, memory and swap.'),
    createClient: () => new HttpHostInfoClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'processes',
    element: 'anth-process-manager',
    icon: '⚙️',
    title: () => msg('Processes'),
    description: () =>
      msg('Live process table with signals, tree view and spawning.'),
    createClient: () => new HttpProcessClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'files',
    element: 'anth-file-manager',
    icon: '📁',
    title: () => msg('Files'),
    description: () =>
      msg('Browse the host filesystem, preview, upload and edit.'),
    createClient: () => new HttpFileClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'network',
    element: 'anth-network-monitor',
    icon: '🌐',
    title: () => msg('Network'),
    description: () => msg('Interfaces, addresses, traffic rates and link state.'),
    createClient: () => new HttpNetworkClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'firewall',
    element: 'anth-firewall',
    icon: '🛡️',
    title: () => msg('Firewall'),
    description: () => msg('Managed nftables chain, allow and deny rules.'),
    createClient: () => new HttpFirewallClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'volumes',
    element: 'anth-volume-manager',
    icon: '💾',
    title: () => msg('Storage'),
    description: () => msg('Block devices, mounts, mounting and unmounting.'),
    createClient: () => new HttpVolumeClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'accounts',
    element: 'anth-account-manager',
    icon: '👤',
    title: () => msg('Accounts'),
    description: () => msg('Local users and groups, locks and passwords.'),
    createClient: () => new HttpAccountClient({ fetchImpl: currentApiFetch() }),
  },
  {
    id: 'terminal',
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

export type { AccountClient, FileClient, FirewallClient, HostInfoClient }
export type { NetworkClient, ProcessClient, TerminalTransport, VolumeClient }
