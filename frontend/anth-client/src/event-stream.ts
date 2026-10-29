/**
 * One server-sent-events stream with the same `next()` surface as the Rust
 * `EventStream<T>`.
 *
 * `EventSource` reconnects on its own, but the panel stream is a snapshot feed:
 * a failed connection is reported and closed so the caller can decide whether
 * to reconnect. The async iterator is a convenience on top of `next()`.
 */

import { WireError } from './http'

export class EventStream<T> implements AsyncIterable<T> {
  private readonly source: EventSource
  private readonly queue: T[] = []
  private readonly waiters: Array<{
    resolve: (value: T | null) => void
    reject: (error: unknown) => void
  }> = []
  private closed = false
  private error: unknown = null

  constructor(
    url: string,
    event: string,
    parse: (value: unknown) => T = (value) => value as T,
  ) {
    this.source = new EventSource(url)
    this.source.addEventListener(event, (message) => {
      try {
        this.push(parse(JSON.parse((message as MessageEvent<string>).data)))
      } catch (error) {
        this.fail(error)
      }
    })
    this.source.onerror = () => {
      this.fail(
        new WireError('internal', 'stream', 'the event stream is unavailable'),
      )
    }
  }

  /** Reads the next event, or `null` once the stream is closed. */
  next(): Promise<T | null> {
    if (this.queue.length > 0) {
      return Promise.resolve(this.queue.shift() ?? null)
    }
    if (this.error) {
      return Promise.reject(this.error)
    }
    if (this.closed) {
      return Promise.resolve(null)
    }
    return new Promise<T | null>((resolve, reject) => {
      this.waiters.push({ resolve, reject })
    })
  }

  /** Stops the stream and resolves a waiting `next()` with `null`. */
  close(): void {
    if (this.closed && !this.error) {
      return
    }
    this.closed = true
    this.source.close()
    for (const waiter of this.waiters.splice(0)) {
      waiter.resolve(null)
    }
  }

  async *[Symbol.asyncIterator](): AsyncIterator<T> {
    for (;;) {
      const value = await this.next()
      if (value === null) {
        return
      }
      yield value
    }
  }

  private push(value: T): void {
    const waiter = this.waiters.shift()
    if (waiter) {
      waiter.resolve(value)
    } else {
      this.queue.push(value)
    }
  }

  private fail(error: unknown): void {
    if (this.closed || this.error) {
      return
    }
    this.error = error
    this.source.close()
    for (const waiter of this.waiters.splice(0)) {
      waiter.reject(error)
    }
  }
}
