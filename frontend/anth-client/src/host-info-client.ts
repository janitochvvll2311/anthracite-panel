import { responseError } from './http'
import type { HostInfo, HostInfoHandlers, HostInfoClient } from 'anth-wire'

export class HttpHostInfoClient implements HostInfoClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<HostInfo> {
    const response = await this.fetchImpl('/api/host-info')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as HostInfo
  }

  watch(intervalMs: number, handlers: HostInfoHandlers): () => void {
    const source = new EventSource(`/api/host-info/watch?interval=${intervalMs}ms`)
    handlers.onStatus?.('connecting')

    const onSnapshot = (event: MessageEvent<string>) => {
      handlers.onStatus?.('live')
      try {
        handlers.onSnapshot(JSON.parse(event.data) as HostInfo)
      } catch (error) {
        handlers.onError(error instanceof Error ? error : new Error(String(error)))
      }
    }

    source.addEventListener('host-info', onSnapshot as EventListener)
    source.onerror = () => {
      handlers.onStatus?.('closed')
      handlers.onError(new Error('the host-info stream is unavailable'))
      source.close()
    }

    return () => {
      handlers.onStatus?.('closed')
      source.close()
    }
  }
}
