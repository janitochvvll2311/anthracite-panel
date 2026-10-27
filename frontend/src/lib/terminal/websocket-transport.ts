import { JsonWebSocketTransport } from '../ws/json-websocket'
import {
  decodeBase64,
  encodeBase64,
  parseTerminalServerMessage,
  type TerminalClientMessage,
  type TerminalServerMessage,
} from './protocol'
import type {
  TerminalSessionInfo,
  TerminalSize,
  TerminalStatus,
  TerminalTransport,
  TerminalTransportHandlers,
} from './transport'

export interface WebSocketTerminalTransportOptions {
  /** Endpoint URL, or a function that receives the requested initial size. */
  url: string | ((size: TerminalSize) => string)
  /** Application-level ping cadence. `0` disables it. */
  heartbeatIntervalMs?: number
  /** How long to wait for `ready`, including the WebSocket upgrade. */
  openTimeoutMs?: number
}

/**
 * `TerminalTransport` backed by the panel's JSON WebSocket endpoint.
 *
 * The wire format is the one documented in `docs/api-endpoints.md`: input and
 * output travel as base64 inside JSON text frames, and `ready` precedes any
 * output. The component never sees any of that.
 */
export class WebSocketTerminalTransport implements TerminalTransport {
  private readonly url: WebSocketTerminalTransportOptions['url']
  private readonly heartbeatIntervalMs: number
  private readonly openTimeoutMs: number
  private socket?: JsonWebSocketTransport<
    TerminalClientMessage,
    TerminalServerMessage
  >
  private handlers?: TerminalTransportHandlers
  private pendingOpen?: {
    resolve: (info: TerminalSessionInfo) => void
    reject: (error: Error) => void
  }
  private heartbeat?: ReturnType<typeof setInterval>
  private openTimer?: ReturnType<typeof setTimeout>
  private intentionalClose = false
  private lastStatus?: TerminalStatus

  constructor(options: WebSocketTerminalTransportOptions) {
    this.url = options.url
    this.heartbeatIntervalMs = options.heartbeatIntervalMs ?? 20_000
    this.openTimeoutMs = options.openTimeoutMs ?? 10_000
  }

  async open(
    size: TerminalSize,
    handlers: TerminalTransportHandlers,
  ): Promise<TerminalSessionInfo> {
    this.close()
    this.handlers = handlers
    this.intentionalClose = false
    this.setStatus('connecting')

    const url =
      typeof this.url === 'function' ? this.url(size) : this.url

    const ready = new Promise<TerminalSessionInfo>((resolve, reject) => {
      this.pendingOpen = { resolve, reject }
    })
    // `connect()` can reject before the caller receives this promise; keep its
    // rejection from surfacing as an unhandled rejection.
    void ready.catch(() => {})
    this.openTimer = setTimeout(() => {
      if (this.pendingOpen) {
        this.reportError(new Error('timed out connecting to the terminal'))
      }
    }, this.openTimeoutMs)

    const socket = new JsonWebSocketTransport<
      TerminalClientMessage,
      TerminalServerMessage
    >({
      url,
      deserialize: (text) =>
        parseTerminalServerMessage(JSON.parse(text) as unknown),
      onMessage: (message) => this.handleMessage(socket, message),
      onClose: (event) => this.handleClose(socket, event),
      onMessageError: (error) => this.reportError(error),
    })
    this.socket = socket

    try {
      await socket.connect()
    } catch (error) {
      const wrapped =
        error instanceof Error ? error : new Error(String(error))
      this.intentionalClose = true
      this.pendingOpen = undefined
      this.clearOpenTimer()
      this.closeSocket()
      this.setStatus('error')
      throw wrapped
    }

    return ready
  }

  sendInput(data: Uint8Array): void {
    // A paste can be larger than the daemon's 64 KiB frame cap. Chunk it so a
    // single large paste never becomes one rejected frame.
    const chunkSize = 16 * 1024
    for (let offset = 0; offset < data.length; offset += chunkSize) {
      this.send({
        type: 'input',
        data: encodeBase64(data.subarray(offset, offset + chunkSize)),
      })
    }
  }

  resize(size: TerminalSize): void {
    if (
      !Number.isInteger(size.cols) ||
      !Number.isInteger(size.rows) ||
      size.cols < 1 ||
      size.rows < 1
    ) {
      return
    }
    this.send({ type: 'resize', cols: size.cols, rows: size.rows })
  }

  close(): void {
    const wasActive = this.socket !== undefined || this.pendingOpen !== undefined
    this.intentionalClose = true
    this.pendingOpen?.reject(new Error('terminal closed before it was ready'))
    this.pendingOpen = undefined
    this.clearOpenTimer()
    this.closeSocket()
    if (wasActive) {
      this.setStatus('closed')
    }
  }

  private send(message: TerminalClientMessage): void {
    if (!this.socket || this.socket.state !== 'open') return
    try {
      this.socket.send(message)
    } catch (error) {
      this.reportError(error instanceof Error ? error : new Error(String(error)))
    }
  }

  private handleMessage(
    socket: JsonWebSocketTransport<TerminalClientMessage, TerminalServerMessage>,
    message: TerminalServerMessage,
  ): void {
    if (this.socket !== socket) return

    switch (message.type) {
      case 'ready':
        this.clearOpenTimer()
        this.startHeartbeat()
        this.setStatus('open')
        this.pendingOpen?.resolve({
          program: message.shell,
          size: { cols: message.cols, rows: message.rows },
          lineEditing: message.line_editing,
        })
        this.pendingOpen = undefined
        break
      case 'output':
        this.handlers?.onData(decodeBase64(message.data))
        break
      case 'exit':
        this.handlers?.onExit(message.code)
        this.intentionalClose = true
        this.closeSocket()
        this.setStatus('closed')
        break
      case 'error': {
        this.reportError(new Error(message.detail))
        break
      }
      case 'pong':
        break
    }
  }

  private handleClose(
    socket: JsonWebSocketTransport<TerminalClientMessage, TerminalServerMessage>,
    event: CloseEvent,
  ): void {
    if (this.socket !== socket) return
    this.socket = undefined
    this.stopHeartbeat()
    this.clearOpenTimer()

    if (this.intentionalClose) {
      this.setStatus('closed')
      return
    }

    const reason = event.reason === '' ? '' : `: ${event.reason}`
    this.reportError(
      new Error(`terminal connection closed (code ${event.code}${reason})`),
    )
  }

  private reportError(error: Error): void {
    // While `open()` is still pending, the rejection of that promise is the
    // error signal; only report through the callback once the session is live.
    const opening = this.pendingOpen !== undefined
    this.clearOpenTimer()
    this.pendingOpen?.reject(error)
    this.pendingOpen = undefined
    this.intentionalClose = true
    this.closeSocket()
    if (!opening) {
      this.handlers?.onError(error)
    }
    this.setStatus('error')
  }

  private clearOpenTimer(): void {
    if (this.openTimer !== undefined) {
      clearTimeout(this.openTimer)
      this.openTimer = undefined
    }
  }

  private setStatus(status: TerminalStatus): void {
    if (this.lastStatus === status) return
    this.lastStatus = status
    this.handlers?.onStatus?.(status)
  }

  private closeSocket(): void {
    const socket = this.socket
    this.socket = undefined
    this.stopHeartbeat()
    socket?.close()
  }

  private startHeartbeat(): void {
    this.stopHeartbeat()
    const interval = this.heartbeatIntervalMs
    if (interval <= 0) return

    this.heartbeat = setInterval(() => {
      if (this.socket?.state !== 'open') return
      try {
        this.socket.send({ type: 'ping' })
      } catch {
        // The close handler reports the failure.
      }
    }, interval)
  }

  private stopHeartbeat(): void {
    if (this.heartbeat !== undefined) {
      clearInterval(this.heartbeat)
      this.heartbeat = undefined
    }
  }
}
