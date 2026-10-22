/**
 * HTTP/SSE client for the host-info endpoints.
 *
 * The component layer never talks to fetch or EventSource directly, so a
 * different backend can implement the same interface.
 */

import { responseError } from '../http'
import { watchJson } from '../stream'
import type { HttpStreamClientOptions } from '../http'
import type { StreamConnectionStatus, StreamHandlers } from '../stream'

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

export type HostInfoConnectionStatus = StreamConnectionStatus

export interface HostInfoHandlers
  extends StreamHandlers<HostInfo> {}

export interface HostInfoClient {
  get: () => Promise<HostInfo>
  watch: (intervalMs: number, handlers: HostInfoHandlers) => () => void
}

export interface HttpHostInfoClientOptions extends HttpStreamClientOptions {}

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
    return watchJson({
      baseUrl: this.baseUrl,
      event: 'host-info',
      intervalMs,
      disconnectedMessage: 'host info stream disconnected',
      handlers,
      EventSourceImpl: this.EventSourceImpl,
    })
  }
}
