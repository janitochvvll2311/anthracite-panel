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
