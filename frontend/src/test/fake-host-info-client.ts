import { vi } from 'vitest'
import type {
  HostInfo,
  HostInfoClient,
  HostInfoHandlers,
} from '../lib/hostinfo/host-info-client'

export function hostInfo(overrides: Partial<HostInfo> = {}): HostInfo {
  return {
    hostname: 'node-1',
    os: {
      name: 'Debian GNU/Linux',
      version: '13',
      kernel: '6.12.0-amd64',
      arch: 'x86_64',
    },
    uptime_seconds: 90_061,
    cpu_model: 'Intel Xeon',
    logical_cores: 4,
    load: { one: 0.5, five: 0.25, fifteen: 0.1 },
    memory: {
      total_kib: 8_388_608,
      available_kib: 4_194_304,
      swap_total_kib: 1_048_576,
      swap_free_kib: 524_288,
    },
    ...overrides,
  }
}

export class FakeHostInfoClient implements HostInfoClient {
  current = hostInfo()
  getError?: Error
  watchCalls: number[] = []
  handlers?: HostInfoHandlers
  unsubscribes = 0

  get = vi.fn(async (): Promise<HostInfo> => {
    if (this.getError) throw this.getError
    return this.current
  })

  watch = vi.fn((intervalMs: number, handlers: HostInfoHandlers) => {
    this.watchCalls.push(intervalMs)
    this.handlers = handlers
    handlers.onStatus?.('connecting')
    return () => {
      this.unsubscribes += 1
      handlers.onStatus?.('closed')
    }
  })

  emit(info: HostInfo): void {
    this.current = info
    this.handlers?.onSnapshot(info)
  }

  status(status: 'live' | 'error' | 'closed'): void {
    this.handlers?.onStatus?.(status)
  }

  fail(error: Error): void {
    this.handlers?.onError?.(error)
  }
}
