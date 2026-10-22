import { vi } from 'vitest'
import type {
  ProcessClient,
  ProcessHandlers,
  ProcessSignal,
  ProcessSnapshot,
  SpawnRequest,
  SpawnedProcess,
} from '../lib/processes/process-client'

export function processSnapshot(
  overrides: Partial<ProcessSnapshot> = {},
): ProcessSnapshot {
  return {
    processes: [],
    clock_ticks: 100,
    timestamp_ms: 1_000,
    control_enabled: false,
    spawn_enabled: false,
    spawn_allow: [],
    total: 0,
    truncated: false,
    ...overrides,
  }
}

export class FakeProcessClient implements ProcessClient {
  current = processSnapshot()
  listError?: Error
  watchCalls: number[] = []
  handlers?: ProcessHandlers
  unsubscribes = 0

  list = vi.fn(async (): Promise<ProcessSnapshot> => {
    if (this.listError) throw this.listError
    return this.current
  })

  watch = vi.fn((intervalMs: number, handlers: ProcessHandlers) => {
    this.watchCalls.push(intervalMs)
    this.handlers = handlers
    handlers.onStatus?.('connecting')
    return () => {
      this.unsubscribes += 1
      handlers.onStatus?.('closed')
    }
  })

  signal = vi.fn(async (_pid: number, _signal: ProcessSignal): Promise<void> => {})

  spawn = vi.fn(
    async (request: SpawnRequest): Promise<SpawnedProcess> => ({
      pid: 4242,
      program: request.program,
      args: request.args ?? [],
    }),
  )

  emit(snapshot: ProcessSnapshot): void {
    this.current = snapshot
    this.handlers?.onSnapshot(snapshot)
  }
}
