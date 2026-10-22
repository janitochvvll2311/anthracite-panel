/** Shared EventSource lifecycle for JSON snapshots. */

export type StreamConnectionStatus =
  | 'idle'
  | 'connecting'
  | 'live'
  | 'error'
  | 'closed'

export interface StreamHandlers<T> {
  onSnapshot: (snapshot: T) => void
  onError?: (error: Error) => void
  onStatus?: (status: StreamConnectionStatus) => void
}

export interface JsonWatchOptions<T> {
  baseUrl: string
  event: string
  intervalMs: number
  disconnectedMessage: string
  handlers: StreamHandlers<T>
  EventSourceImpl: typeof EventSource
}

/**
 * Opens `${baseUrl}/watch?interval=Nms`, parses one named JSON event and
 * reports the shared connection lifecycle. The returned function closes it.
 */
export function watchJson<T>(options: JsonWatchOptions<T>): () => void {
  const interval = Math.max(10, Math.round(options.intervalMs))
  const source = new options.EventSourceImpl(
    `${options.baseUrl}/watch?interval=${interval}ms`,
  )
  const { handlers } = options

  handlers.onStatus?.('connecting')
  source.onopen = () => handlers.onStatus?.('live')
  source.addEventListener(options.event, (event) => {
    try {
      handlers.onSnapshot(
        JSON.parse((event as MessageEvent<string>).data) as T,
      )
    } catch (error) {
      handlers.onError?.(
        error instanceof Error ? error : new Error(String(error)),
      )
    }
  })
  source.onerror = () => {
    handlers.onError?.(new Error(options.disconnectedMessage))
    handlers.onStatus?.('error')
  }

  return () => {
    source.close()
    handlers.onStatus?.('closed')
  }
}
