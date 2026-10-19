/**
 * A small, application-agnostic JSON WebSocket transport.
 *
 * It owns connection state, text-frame JSON encoding/decoding and close
 * handling. It knows nothing about terminals or any particular endpoint, so a
 * second WebSocket feature can reuse it by providing different message types.
 */

export type JsonWebSocketState =
  | 'idle'
  | 'connecting'
  | 'open'
  | 'closing'
  | 'closed'

export interface JsonWebSocketOptions<Rx> {
  /** Absolute URL, or a function evaluated on every connect. */
  url: string | (() => string)
  deserialize?: (data: string) => Rx
  onMessage?: (message: Rx) => void
  onClose?: (event: CloseEvent) => void
  onMessageError?: (error: Error) => void
}

export class JsonWebSocketTransport<Tx, Rx = unknown> {
  private readonly options: JsonWebSocketOptions<Rx>
  private socket?: WebSocket
  private currentState: JsonWebSocketState = 'idle'
  private connectPromise?: Promise<void>
  private pendingConnect?: {
    resolve: () => void
    reject: (error: Error) => void
  }

  constructor(options: JsonWebSocketOptions<Rx>) {
    this.options = options
  }

  get state(): JsonWebSocketState {
    return this.currentState
  }

  async connect(): Promise<void> {
    if (this.currentState === 'open') return
    if (!this.connectPromise) {
      this.connectPromise = this.openSocket().finally(() => {
        this.connectPromise = undefined
      })
    }
    return this.connectPromise
  }

  private openSocket(): Promise<void> {
    this.setState('connecting')
    const url =
      typeof this.options.url === 'function' ? this.options.url() : this.options.url

    return new Promise<void>((resolve, reject) => {
      this.pendingConnect = { resolve, reject }

      let socket: WebSocket
      try {
        socket = new WebSocket(url)
      } catch (error) {
        this.failConnect(error instanceof Error ? error : new Error(String(error)))
        return
      }

      this.socket = socket

      socket.onopen = () => {
        if (this.socket !== socket) return
        this.pendingConnect = undefined
        this.setState('open')
        resolve()
      }

      socket.onmessage = (event) => {
        if (this.socket !== socket) return
        if (typeof event.data !== 'string') {
          this.options.onMessageError?.(
            new Error('expected a text WebSocket frame'),
          )
          return
        }

        try {
          const message = this.options.deserialize
            ? this.options.deserialize(event.data)
            : (JSON.parse(event.data) as Rx)
          this.options.onMessage?.(message)
        } catch (error) {
          this.options.onMessageError?.(
            error instanceof Error ? error : new Error(String(error)),
          )
        }
      }

      socket.onclose = (event) => {
        if (this.socket !== socket) return
        this.socket = undefined
        this.pendingConnect?.reject(
          new Error(`WebSocket closed before opening (code ${event.code})`),
        )
        this.pendingConnect = undefined
        this.setState('closed')
        this.options.onClose?.(event)
      }
    })
  }

  send(message: Tx): void {
    if (this.currentState !== 'open' || !this.socket) {
      throw new Error('WebSocket is not open')
    }

    const data = JSON.stringify(message)
    this.socket.send(data)
  }

  close(code = 1000, reason = ''): void {
    const socket = this.socket
    if (!socket) {
      this.setState('closed')
      return
    }

    this.setState('closing')
    socket.close(code, reason)
  }

  private failConnect(error: Error): void {
    this.pendingConnect?.reject(error)
    this.pendingConnect = undefined
    this.socket = undefined
    this.setState('closed')
  }

  private setState(state: JsonWebSocketState): void {
    if (this.currentState === state) return
    this.currentState = state
  }
}
