import type { Change } from './actions'

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

export interface NetworkEdit extends NetworkEditRequest {
  name: string
}

export interface NetworkClient {
  get(): Promise<NetworkSnapshot>
  /** Repeats the inventory on an interval until the returned function runs. */
  watch(intervalMs: number, handlers: NetworkHandlers): () => void
  /** Brings one interface up or down. Resolves to the daemon message. */
  setState(name: string, up: boolean): Promise<Change>
  create(request: NetworkCreateRequest): Promise<Change>
  edit(request: NetworkEdit): Promise<Change>
  rename(name: string, newName: string): Promise<Change>
  remove(name: string): Promise<Change>
}
