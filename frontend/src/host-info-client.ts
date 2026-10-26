export interface OsInfo {
  name: string
  version: string
  kernel: string
  arch: string
}

export interface LoadAverage {
  one: number
  five: number
  fifteen: number
}

export interface MemoryInfo {
  total_kib: number
  available_kib: number
  swap_total_kib: number
  swap_free_kib: number
}

export interface HostInfo {
  hostname: string
  os: OsInfo
  uptime_seconds: number
  cpu_model: string
  logical_cores: number
  load: LoadAverage
  memory: MemoryInfo
  collected_at_ms: number
}

export interface HostInfoHandlers {
  onSnapshot: (info: HostInfo) => void
  onError: (error: Error) => void
  onStatus?: (status: 'connecting' | 'live' | 'closed') => void
}

export interface HostInfoClient {
  get(): Promise<HostInfo>
  watch(intervalMs: number, handlers: HostInfoHandlers): () => void
}

export class HttpHostInfoClient implements HostInfoClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    this.fetchImpl = fetchImpl
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

async function responseError(response: Response): Promise<Error> {
  let detail = `${response.status} ${response.statusText}`.trim()
  try {
    const body = (await response.json()) as { error?: { message?: unknown } }
    if (typeof body.error?.message === 'string') detail = body.error.message
  } catch {
    // Keep the status text.
  }
  return new Error(detail)
}
