/**
 * HTTP/SSE client for the process endpoints.
 *
 * The component layer never talks to fetch or EventSource directly, so a
 * different backend can implement the same interface.
 */

import { responseError } from '../http'
import { watchJson } from '../stream'
import type { HttpStreamClientOptions } from '../http'
import type { StreamConnectionStatus, StreamHandlers } from '../stream'

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

export type ProcessConnectionStatus = StreamConnectionStatus

export interface ProcessHandlers extends StreamHandlers<ProcessSnapshot> {}

export interface ProcessClient {
  list: () => Promise<ProcessSnapshot>
  watch: (intervalMs: number, handlers: ProcessHandlers) => () => void
  signal: (pid: number, signal: ProcessSignal) => Promise<void>
}

export interface HttpProcessClientOptions extends HttpStreamClientOptions {}

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
    return watchJson({
      baseUrl: this.baseUrl,
      event: 'processes',
      intervalMs,
      disconnectedMessage: 'process stream disconnected',
      handlers,
      EventSourceImpl: this.EventSourceImpl,
    })
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
