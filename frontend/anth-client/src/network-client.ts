import type {
  Change,
  NetworkClient,
  NetworkCreateRequest,
  NetworkEdit,
  NetworkHandlers,
  NetworkSnapshot,
} from 'anth-wire'

import { EventStream } from './event-stream'
import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpNetworkClient implements NetworkClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  get(): Promise<NetworkSnapshot> {
    return this.transport.json<NetworkSnapshot>('GET', '/api/network')
  }

  /** One snapshot per interval, with the same `next()` shape as Rust. */
  stream(intervalMs: number): EventStream<NetworkSnapshot> {
    return new EventStream<NetworkSnapshot>(
      this.transport.url(`/api/network/watch?interval=${intervalMs}ms`),
      'network',
    )
  }

  watch(intervalMs: number, handlers: NetworkHandlers): () => void {
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

  setState(name: string, up: boolean): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/network/state', {
      name,
      up,
    })
  }

  create(request: NetworkCreateRequest): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/network/create', request)
  }

  edit(request: NetworkEdit): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/network/edit', request)
  }

  rename(name: string, newName: string): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/network/rename', {
      name,
      new_name: newName,
    })
  }

  remove(name: string): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/network/delete', {
      name,
    })
  }
}
