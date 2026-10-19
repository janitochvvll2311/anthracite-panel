/**
 * HTTP/SSE client for the host-info endpoints.
 *
 * The component layer never talks to fetch or EventSource directly, so a
 * different backend can implement the same interface.
 */

export interface HostOsInfo {
  name: string
  version: string
  kernel: string
  arch: string
}

export interface HostLoadAverage {
  one: number
  five: number
  fifteen: number
}

export interface HostMemoryInfo {
  total_kib: number
  available_kib: number
  swap_total_kib: number
  swap_free_kib: number
}

export interface HostInfo {
  hostname: string
  os: HostOsInfo
  uptime_seconds: number
  cpu_model: string
  logical_cores: number
  load: HostLoadAverage
  memory: HostMemoryInfo
}

export type HostInfoConnectionStatus =
  | 'idle'
  | 'connecting'
  | 'live'
  | 'error'
  | 'closed'

export interface HostInfoHandlers {
  onSnapshot: (info: HostInfo) => void
  onError?: (error: Error) => void
  onStatus?: (status: HostInfoConnectionStatus) => void
}

export interface HostInfoClient {
  get: () => Promise<HostInfo>
  watch: (intervalMs: number, handlers: HostInfoHandlers) => () => void
}

export interface HttpHostInfoClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
  EventSourceImpl?: typeof EventSource
}

export class HttpHostInfoClient implements HostInfoClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch
  private readonly EventSourceImpl: typeof EventSource

  constructor(options: HttpHostInfoClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api/host-info'
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
    this.EventSourceImpl = options.EventSourceImpl ?? globalThis.EventSource
  }

  async get(): Promise<HostInfo> {
    const response = await this.fetchImpl(this.baseUrl, {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as HostInfo
  }

  watch(intervalMs: number, handlers: HostInfoHandlers): () => void {
    const interval = Math.max(10, Math.round(intervalMs))
    const source = new this.EventSourceImpl(
      `${this.baseUrl}/watch?interval=${interval}ms`,
    )

    handlers.onStatus?.('connecting')
    source.onopen = () => handlers.onStatus?.('live')
    source.addEventListener('host-info', (event) => {
      try {
        handlers.onSnapshot(
          JSON.parse((event as MessageEvent<string>).data) as HostInfo,
        )
      } catch (error) {
        handlers.onError?.(
          error instanceof Error ? error : new Error(String(error)),
        )
      }
    })
    source.onerror = () => {
      handlers.onError?.(new Error('host info stream disconnected'))
      handlers.onStatus?.('error')
    }

    return () => {
      source.close()
      handlers.onStatus?.('closed')
    }
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
