import type {
  ProcessSignal,
  ProcessSignalled,
  ProcessSnapshot,
  ProcessStarted,
  ProcessesClient,
  ProcessesHandlers,
} from 'anth-wire'

import { EventStream } from './event-stream'
import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

/** Rows one live page asks for; the API caps a page at 1000. */
const LIVE_LIMIT = 1000

export class HttpProcessesClient implements ProcessesClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  list(limit = LIVE_LIMIT, cursor?: string): Promise<ProcessSnapshot> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (cursor) {
      query.set('cursor', cursor)
    }
    return this.transport.json<ProcessSnapshot>(
      'GET',
      `/api/processes?${query}`,
    )
  }

  /** One snapshot per interval, with the same `next()` shape as Rust. */
  stream(
    intervalMs: number,
    limit = LIVE_LIMIT,
  ): EventStream<ProcessSnapshot> {
    return new EventStream<ProcessSnapshot>(
      this.transport.url(
        `/api/processes/watch?interval=${intervalMs}ms&limit=${limit}`,
      ),
      'processes',
    )
  }

  watch(intervalMs: number, handlers: ProcessesHandlers): () => void {
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

  signal(
    pid: number,
    startTicks: number,
    signal: ProcessSignal,
    group = false,
  ): Promise<ProcessSignalled> {
    return this.transport.json<ProcessSignalled>(
      'POST',
      '/api/processes/signal',
      {
        pid,
        start_ticks: startTicks,
        signal,
        group,
      },
    )
  }

  spawn(
    program: string,
    args: string[],
    cwd?: string,
  ): Promise<ProcessStarted> {
    return this.transport.json<ProcessStarted>(
      'POST',
      '/api/processes/spawn',
      {
        program,
        args,
        ...(cwd ? { cwd } : {}),
      },
    )
  }
}
