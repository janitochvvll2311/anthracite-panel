import { responseError } from './http'
import type { NetworkSnapshot, NetworkHandlers, NetworkCreateRequest, NetworkEditRequest, NetworkClient } from 'anth-wire'

export class HttpNetworkClient implements NetworkClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<NetworkSnapshot> {
    const response = await this.fetchImpl('/api/network')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as NetworkSnapshot
  }

  watch(intervalMs: number, handlers: NetworkHandlers): () => void {
    const source = new EventSource(`/api/network/watch?interval=${intervalMs}ms`)
    handlers.onStatus?.('connecting')

    const onSnapshot = (event: MessageEvent<string>) => {
      handlers.onStatus?.('live')
      try {
        handlers.onSnapshot(JSON.parse(event.data) as NetworkSnapshot)
      } catch (error) {
        handlers.onError(error instanceof Error ? error : new Error(String(error)))
      }
    }

    source.addEventListener('network', onSnapshot as EventListener)
    source.onerror = () => {
      handlers.onStatus?.('closed')
      handlers.onError(new Error('the network stream is unavailable'))
      source.close()
    }

    return () => {
      handlers.onStatus?.('closed')
      source.close()
    }
  }

  async setState(name: string, up: boolean): Promise<string> {
    return this.action('/api/network/state', { name, up })
  }

  async create(request: NetworkCreateRequest): Promise<string> {
    return this.action('/api/network/create', request)
  }

  async edit(name: string, request: NetworkEditRequest): Promise<string> {
    return this.action('/api/network/edit', { name, ...request })
  }

  async rename(name: string, newName: string): Promise<string> {
    return this.action('/api/network/rename', { name, new_name: newName })
  }

  async remove(name: string): Promise<string> {
    return this.action('/api/network/delete', { name })
  }

  private async action(path: string, body: unknown): Promise<string> {
    const response = await this.fetchImpl(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await responseError(response)
    const result = (await response.json()) as { message?: string }
    return result.message ?? ''
  }
}
