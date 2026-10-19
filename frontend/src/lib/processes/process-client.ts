/**
 * HTTP/SSE client for the process endpoints.
 *
 * The component layer never talks to fetch or EventSource directly, so a
 * different backend can implement the same interface.
 */

export type ProcessSignal =
  | 'term'
  | 'kill'
  | 'hup'
  | 'int'
  | 'quit'
  | 'stop'
  | 'cont'

export interface ProcessInfo {
  pid: number
  ppid: number
  uid: number
  user: string
  name: string
  command: string
  state: string
  cpu_ticks: number
  rss_kib: number
  threads: number
  start_ticks: number
}

export interface ProcessSnapshot {
  processes: ProcessInfo[]
  clock_ticks: number
  timestamp_ms: number
  control_enabled: boolean
  total: number
  truncated: boolean
  warning?: string
}

/** A process plus the percentage computed from the previous snapshot. */
export interface ProcessRow extends ProcessInfo {
  cpu_percent: number | null
}

export type ProcessConnectionStatus =
  | 'idle'
  | 'connecting'
  | 'live'
  | 'error'
  | 'closed'

export interface ProcessHandlers {
  onSnapshot: (snapshot: ProcessSnapshot) => void
  onError?: (error: Error) => void
  onStatus?: (status: ProcessConnectionStatus) => void
}

export interface ProcessClient {
  list: () => Promise<ProcessSnapshot>
  watch: (intervalMs: number, handlers: ProcessHandlers) => () => void
  signal: (pid: number, signal: ProcessSignal) => Promise<void>
}

export interface HttpProcessClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
  EventSourceImpl?: typeof EventSource
}

export class HttpProcessClient implements ProcessClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch
  private readonly EventSourceImpl: typeof EventSource

  constructor(options: HttpProcessClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api/processes'
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
    this.EventSourceImpl = options.EventSourceImpl ?? globalThis.EventSource
  }

  async list(): Promise<ProcessSnapshot> {
    const response = await this.fetchImpl(this.baseUrl, {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) {
      throw await responseError(response)
    }
    return (await response.json()) as ProcessSnapshot
  }

  watch(intervalMs: number, handlers: ProcessHandlers): () => void {
    const interval = Math.max(10, Math.round(intervalMs))
    const source = new this.EventSourceImpl(
      `${this.baseUrl}/watch?interval=${interval}ms`,
    )

    handlers.onStatus?.('connecting')
    source.onopen = () => handlers.onStatus?.('live')
    source.addEventListener('processes', (event) => {
      try {
        handlers.onSnapshot(
          JSON.parse((event as MessageEvent<string>).data) as ProcessSnapshot,
        )
      } catch (error) {
        handlers.onError?.(
          error instanceof Error ? error : new Error(String(error)),
        )
      }
    })
    source.onerror = () => {
      handlers.onError?.(new Error('process stream disconnected'))
      handlers.onStatus?.('error')
    }

    return () => {
      source.close()
      handlers.onStatus?.('closed')
    }
  }

  async signal(pid: number, signal: ProcessSignal): Promise<void> {
    const response = await this.fetchImpl(`${this.baseUrl}/${pid}/signal`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ signal }),
    })

    if (!response.ok) {
      throw await responseError(response)
    }
  }
}

async function responseError(response: Response): Promise<Error> {
  let detail = `${response.status} ${response.statusText}`.trim()

  try {
    const body = (await response.json()) as { error?: unknown }
    if (typeof body.error === 'string') {
      detail = body.error
    }
  } catch {
    // The body was not JSON; keep the status text.
  }

  return new Error(detail)
}
