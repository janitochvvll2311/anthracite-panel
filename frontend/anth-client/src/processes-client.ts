import { responseError } from './http'
/** Rows one live page asks for; the API caps a page at 1000. */
const LIVE_LIMIT = 1000

import type { ProcessSnapshot, ProcessSignal, ProcessStarted, ProcessesHandlers, ProcessesClient } from 'anth-wire'

export class HttpProcessesClient implements ProcessesClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async list(limit = LIVE_LIMIT, cursor?: string): Promise<ProcessSnapshot> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (cursor) {
      query.set('cursor', cursor)
    }
    const response = await this.fetchImpl(`/api/processes?${query}`)
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as ProcessSnapshot
  }

  watch(intervalMs: number, handlers: ProcessesHandlers): () => void {
    const source = new EventSource(
      `/api/processes/watch?interval=${intervalMs}ms&limit=${LIVE_LIMIT}`,
    )
    handlers.onStatus?.('connecting')

    const onSnapshot = (event: MessageEvent<string>) => {
      handlers.onStatus?.('live')
      try {
        handlers.onSnapshot(JSON.parse(event.data) as ProcessSnapshot)
      } catch (error) {
        handlers.onError(error instanceof Error ? error : new Error(String(error)))
      }
    }

    source.addEventListener('processes', onSnapshot as EventListener)
    source.onerror = () => {
      handlers.onStatus?.('closed')
      handlers.onError(new Error('the process stream is unavailable'))
      source.close()
    }

    return () => {
      handlers.onStatus?.('closed')
      source.close()
    }
  }

  async signal(
    pid: number,
    startTicks: number,
    signal: ProcessSignal,
    group = false,
  ): Promise<void> {
    const response = await this.fetchImpl('/api/processes/signal', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        pid,
        start_ticks: startTicks,
        signal,
        group,
      }),
    })
    if (!response.ok) throw await responseError(response)
  }

  async spawn(
    program: string,
    args: string[],
    cwd?: string,
  ): Promise<ProcessStarted> {
    const response = await this.fetchImpl('/api/processes/spawn', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        program,
        args,
        ...(cwd ? { cwd } : {}),
      }),
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as ProcessStarted
  }
}
