/**
 * HTTP/SSE client for the network endpoints.
 *
 * The component layer never talks to fetch or EventSource directly, so a
 * different backend can implement the same interface.
 */

import { responseError } from '../http'
import { watchJson } from '../stream'
import type { HttpStreamClientOptions } from '../http'
import type { StreamConnectionStatus, StreamHandlers } from '../stream'

export type NetworkAddressFamily = 'ipv4' | 'ipv6'

export interface NetworkAddress {
  family: NetworkAddressFamily
  address: string
  prefix_len: number
}

export interface NetworkInterface {
  name: string
  index: number
  state: string
  up: boolean
  running: boolean
  loopback: boolean
  mac: string
  mtu: number
  speed_mbps: number | null
  duplex: string | null
  addresses: NetworkAddress[]
  rx_bytes: number
  tx_bytes: number
  rx_packets: number
  tx_packets: number
  rx_errors: number
  tx_errors: number
  rx_dropped: number
  tx_dropped: number
}

export interface NetworkSnapshot {
  interfaces: NetworkInterface[]
  timestamp_ms: number
  control_enabled: boolean
}

/** An interface plus the rates computed from the previous snapshot. */
export interface NetworkRow extends NetworkInterface {
  rx_rate: number | null
  tx_rate: number | null
}

export type NetworkConnectionStatus = StreamConnectionStatus

export interface NetworkHandlers
  extends StreamHandlers<NetworkSnapshot> {}

export interface NetworkClient {
  list: () => Promise<NetworkSnapshot>
  watch: (intervalMs: number, handlers: NetworkHandlers) => () => void
  setState: (name: string, up: boolean) => Promise<void>
}

export interface HttpNetworkClientOptions extends HttpStreamClientOptions {}

export class HttpNetworkClient implements NetworkClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch
  private readonly EventSourceImpl: typeof EventSource

  constructor(options: HttpNetworkClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api/network'
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
    this.EventSourceImpl = options.EventSourceImpl ?? globalThis.EventSource
  }

  async list(): Promise<NetworkSnapshot> {
    const response = await this.fetchImpl(this.baseUrl, {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as NetworkSnapshot
  }

  watch(intervalMs: number, handlers: NetworkHandlers): () => void {
    return watchJson({
      baseUrl: this.baseUrl,
      event: 'network',
      intervalMs,
      disconnectedMessage: 'network stream disconnected',
      handlers,
      EventSourceImpl: this.EventSourceImpl,
    })
  }

  async setState(name: string, up: boolean): Promise<void> {
    const response = await this.fetchImpl(
      `${this.baseUrl}/${encodeURIComponent(name)}/state`,
      {
        method: 'POST',
        headers: {
          accept: 'application/json',
          'content-type': 'application/json',
        },
        body: JSON.stringify({ up }),
      },
    )

    if (!response.ok) throw await responseError(response)
  }
}
