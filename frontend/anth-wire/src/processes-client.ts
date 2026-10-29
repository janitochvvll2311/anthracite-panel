import type { ProcessSignalled } from './actions'

/** One process row from `/api/processes`. */
export interface ProcessInfo {
  pid: number
  ppid: number
  uid: number
  user: string
  name: string
  command: string
  state: string
  cpu_ticks: number
  rss_kib: number
  threads: number
  /** Start time in clock ticks; with `pid`, the stable identity. */
  start_ticks: number
}

/** One page plus the facts needed to read it. */
export interface ProcessSnapshot {
  items: ProcessInfo[]
  next?: string | null
  total?: number | null
  clock_ticks: number
  timestamp_ms: number
  control_enabled: boolean
  spawn_enabled: boolean
  spawn_allow: string[]
}

export type ProcessSignal =
  | 'term'
  | 'kill'
  | 'hup'
  | 'int'
  | 'quit'
  | 'stop'
  | 'cont'

/** The signals the action bar offers, in order. */
export const PROCESS_SIGNALS: ProcessSignal[] = [
  'term',
  'kill',
  'hup',
  'int',
  'quit',
  'stop',
  'cont',
]

export interface ProcessStarted {
  pid: number
  program: string
  args: string[]
}

export interface ProcessesHandlers {
  onSnapshot: (snapshot: ProcessSnapshot) => void
  onError: (error: Error) => void
  onStatus?: (status: 'connecting' | 'live' | 'closed') => void
}

export interface ProcessesClient {
  /** One page, busiest first. */
  list(limit?: number, cursor?: string): Promise<ProcessSnapshot>
  /** Repeats the first page on an interval until the returned function runs. */
  watch(intervalMs: number, handlers: ProcessesHandlers): () => void
  signal(
    pid: number,
    startTicks: number,
    signal: ProcessSignal,
    group?: boolean,
  ): Promise<ProcessSignalled>
  spawn(program: string, args: string[], cwd?: string): Promise<ProcessStarted>
}
