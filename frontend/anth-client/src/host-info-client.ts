import type {
  HostInfo,
  HostInfoClient,
  HostInfoHandlers,
} from 'anth-wire'

import { EventStream } from './event-stream'
import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpHostInfoClient implements HostInfoClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  get(): Promise<HostInfo> {
    return this.transport.json<HostInfo>('GET', '/api/host-info')
  }

  /** One snapshot per interval, with the same `next()` shape as Rust. */
  stream(intervalMs: number): EventStream<HostInfo> {
    return new EventStream<HostInfo>(
      this.transport.url(`/api/host-info/watch?interval=${intervalMs}ms`),
      'host-info',
    )
  }

  watch(intervalMs: number, handlers: HostInfoHandlers): () => void {
    const stream = this.stream(intervalMs)
    handlers.onStatus?.('connecting')
    void (async () => {
      try {
        for await (const snapshot of stream) {
          handlers.onStatus?.('live')
          handlers.onSnapshot(snapshot)
        }
      } catch (error) {
        handlers.onError(
          error instanceof Error ? error : new Error(String(error)),
        )
      } finally {
        handlers.onStatus?.('closed')
      }
    })()
    return () => stream.close()
  }
}
