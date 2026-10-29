import { responseError } from './http'

/** One address assigned to an interface. */
export interface NetworkAddress {
  family: 'inet' | 'inet6'
  address: string
  /** Prefix length, not a dotted netmask. */
  prefix: number
  scope: 'global' | 'link' | 'host'
}

export type NetworkInterfaceKind =
  | 'loopback'
  | 'ether'
  | 'wifi'
  | 'bridge'
  | 'tun'
  | 'veth'
  | 'other'

/** One network interface as `/api/network` reports it. */
export interface NetworkInterface {
  name: string
  index: number
  kind: NetworkInterfaceKind
  mac: string
  mtu: number
  /** `/sys/class/net/<name>/operstate`. */
  state: string
  up: boolean
  /** `IFF_RUNNING`; a carrier is present. */
  running: boolean
  loopback: boolean
  /** Real hardware: the interface has a `device` link in sysfs. */
  physical: boolean
  addresses: NetworkAddress[]
  rx_bytes: number
  tx_bytes: number
  rx_packets: number
  tx_packets: number
  rx_errors: number
  tx_errors: number
  rx_dropped: number
  tx_dropped: number
  speed_mbps?: number | null
}

/** A default route the daemon can see. */
export interface NetworkRoute {
  family: 'inet' | 'inet6'
  gateway: string
  interface: string
}

export interface NetworkSnapshot {
  interfaces: NetworkInterface[]
  routes: NetworkRoute[]
  timestamp_ms: number
  /** Whether `[network] control` allows up/down. */
  control_enabled: boolean
  /** Whether `[network] manage` allows create/edit/rename/delete. */
  manage_enabled: boolean
}

export interface NetworkHandlers {
  onSnapshot: (snapshot: NetworkSnapshot) => void
  onError: (error: Error) => void
  onStatus?: (status: 'connecting' | 'live' | 'closed') => void
}

/** The virtual kinds the panel can create. */
export type NetworkCreateKind =
  | 'veth'
  | 'bridge'
  | 'bond'
  | 'vlan'
  | 'dummy'
  | 'tun'

export interface NetworkCreateRequest {
  name: string
  kind: NetworkCreateKind
  peer?: string
  link?: string
  vlan_id?: number
  mtu?: number
  up?: boolean
}

export interface NetworkEditRequest {
  mtu?: number
  up?: boolean
}

export interface NetworkClient {
  get(): Promise<NetworkSnapshot>
  /** Repeats the inventory on an interval until the returned function runs. */
  watch(intervalMs: number, handlers: NetworkHandlers): () => void
  /** Brings one interface up or down. Resolves to the daemon message. */
  setState(name: string, up: boolean): Promise<string>
  create(request: NetworkCreateRequest): Promise<string>
  edit(name: string, request: NetworkEditRequest): Promise<string>
  rename(name: string, newName: string): Promise<string>
  remove(name: string): Promise<string>
}

export class HttpNetworkClient implements NetworkClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<NetworkSnapshot> {
    const response = await this.fetchImpl('/api/network')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as NetworkSnapshot
  }

  watch(intervalMs: number, handlers: NetworkHandlers): () => void {
    const source = new EventSource(`/api/network/watch?interval=${intervalMs}ms`)
    handlers.onStatus?.('connecting')

    const onSnapshot = (event: MessageEvent<string>) => {
      handlers.onStatus?.('live')
      try {
        handlers.onSnapshot(JSON.parse(event.data) as NetworkSnapshot)
      } catch (error) {
        handlers.onError(error instanceof Error ? error : new Error(String(error)))
      }
    }

    source.addEventListener('network', onSnapshot as EventListener)
    source.onerror = () => {
      handlers.onStatus?.('closed')
      handlers.onError(new Error('the network stream is unavailable'))
      source.close()
    }

    return () => {
      handlers.onStatus?.('closed')
      source.close()
    }
  }

  async setState(name: string, up: boolean): Promise<string> {
    return this.action('/api/network/state', { name, up })
  }

  async create(request: NetworkCreateRequest): Promise<string> {
    return this.action('/api/network/create', request)
  }

  async edit(name: string, request: NetworkEditRequest): Promise<string> {
    return this.action('/api/network/edit', { name, ...request })
  }

  async rename(name: string, newName: string): Promise<string> {
    return this.action('/api/network/rename', { name, new_name: newName })
  }

  async remove(name: string): Promise<string> {
    return this.action('/api/network/delete', { name })
  }

  private async action(path: string, body: unknown): Promise<string> {
    const response = await this.fetchImpl(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await responseError(response)
    const result = (await response.json()) as { message?: string }
    return result.message ?? ''
  }
}
