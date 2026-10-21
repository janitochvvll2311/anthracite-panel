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
  /** Whether the daemon accepts a spawn request at all. */
  spawn_enabled: boolean
  /**
   * Absolute programs the daemon will start. Empty denies every spawn. A
   * daemon predating the field omits it, so treat absence as empty.
   */
  spawn_allow?: string[]
  total: number
  truncated: boolean
  warning?: string
}

export interface SpawnRequest {
  program: string
  args?: string[]
  cwd?: string
}

export interface SpawnedProcess {
  pid: number
  program: string
  args: string[]
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
  spawn: (request: SpawnRequest) => Promise<SpawnedProcess>
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

  async spawn(request: SpawnRequest): Promise<SpawnedProcess> {
    const response = await this.fetchImpl(`${this.baseUrl}/spawn`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      // `args` and `cwd` are omitted rather than sent as null so the daemon's
      // `#[serde(default)]` applies instead of a null-into-Vec mismatch.
      body: JSON.stringify({
        program: request.program,
        ...(request.args === undefined ? {} : { args: request.args }),
        ...(request.cwd === undefined ? {} : { cwd: request.cwd }),
      }),
    })

    if (!response.ok) {
      throw await responseError(response)
    }

    return (await response.json()) as SpawnedProcess
  }
}
