/**
 * HTTP/SSE client for the network endpoints.
 *
 * The component layer never talks to fetch or EventSource directly, so a
 * different backend can implement the same interface.
 */

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

export type NetworkConnectionStatus =
  | 'idle'
  | 'connecting'
  | 'live'
  | 'error'
  | 'closed'

export interface NetworkHandlers {
  onSnapshot: (snapshot: NetworkSnapshot) => void
  onError?: (error: Error) => void
  onStatus?: (status: NetworkConnectionStatus) => void
}

export interface NetworkClient {
  list: () => Promise<NetworkSnapshot>
  watch: (intervalMs: number, handlers: NetworkHandlers) => () => void
  setState: (name: string, up: boolean) => Promise<void>
}

export interface HttpNetworkClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
  EventSourceImpl?: typeof EventSource
}

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
    const interval = Math.max(10, Math.round(intervalMs))
    const source = new this.EventSourceImpl(
      `${this.baseUrl}/watch?interval=${interval}ms`,
    )

    handlers.onStatus?.('connecting')
    source.onopen = () => handlers.onStatus?.('live')
    source.addEventListener('network', (event) => {
      try {
        handlers.onSnapshot(
          JSON.parse((event as MessageEvent<string>).data) as NetworkSnapshot,
        )
      } catch (error) {
        handlers.onError?.(
          error instanceof Error ? error : new Error(String(error)),
        )
      }
    })
    source.onerror = () => {
      handlers.onError?.(new Error('network stream disconnected'))
      handlers.onStatus?.('error')
    }

    return () => {
      source.close()
      handlers.onStatus?.('closed')
    }
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

async function responseError(response: Response): Promise<Error> {
  let detail = `${response.status} ${response.statusText}`.trim()

  try {
    const body = (await response.json()) as { error?: unknown }
    if (typeof body.error === 'string') detail = body.error
  } catch {
    // The body was not JSON; keep the status text.
  }

  return new Error(detail)
}
