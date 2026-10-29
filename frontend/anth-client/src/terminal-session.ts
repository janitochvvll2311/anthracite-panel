/**
 * One terminal session over the WebSocket endpoint, with the same `input`,
 * `resize`, `next` and `close` surface as the Rust `TerminalSession`.
 *
 * A browser `WebSocket` cannot set an `Authorization` header, so a deployment
 * that uses bearer tokens instead of cookies must keep the panel on the same
 * origin. The Rust client does not have that restriction.
 */

import {
  encodeBase64,
  parseTerminalServerMessage,
  type TerminalClientMessage,
  type TerminalServerMessage,
  type TerminalSize,
} from 'anth-wire'

import { WireError } from './http'

/** Input chunk carried by one frame; the daemon caps decoded input at 16 KiB. */
const INPUT_CHUNK = 16 * 1024

export class TerminalSession implements AsyncIterable<TerminalServerMessage> {
  private readonly socket: WebSocket
  private readonly queue: TerminalServerMessage[] = []
  private readonly waiters: Array<{
    resolve: (value: TerminalServerMessage | null) => void
    reject: (error: unknown) => void
  }> = []
  private closed = false
  private error: unknown = null

  private constructor(socket: WebSocket) {
    this.socket = socket
  }

  /** Connects and resolves once the WebSocket is open. */
  static open(url: string): Promise<TerminalSession> {
    return new Promise<TerminalSession>((resolve, reject) => {
      const socket = new WebSocket(url)
      const session = new TerminalSession(socket)
      let settled = false

      socket.onopen = () => {
        settled = true
        resolve(session)
      }
      socket.onmessage = (event) => {
        try {
          session.push(
            parseTerminalServerMessage(JSON.parse(String(event.data))),
          )
        } catch (error) {
          session.fail(error)
        }
      }
      socket.onerror = () => {
        const error = new WireError(
          'internal',
          'terminal',
          'could not open the terminal',
        )
        session.fail(error)
        if (!settled) {
          settled = true
          reject(error)
        }
      }
      socket.onclose = () => session.finish()
    })
  }

  /** Sends raw keystrokes, chunked to the daemon's input cap. */
  input(data: Uint8Array): void {
    for (let offset = 0; offset < data.length; offset += INPUT_CHUNK) {
      this.send({
        type: 'input',
        data: encodeBase64(data.subarray(offset, offset + INPUT_CHUNK)),
      })
    }
  }

  /** Sends text as UTF-8 keystrokes. */
  inputText(text: string): void {
    this.input(new TextEncoder().encode(text))
  }

  /** Updates the window size. */
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

  /** Reads the next message, or `null` once the socket closes. */
  next(): Promise<TerminalServerMessage | null> {
    if (this.queue.length > 0) {
      return Promise.resolve(this.queue.shift() ?? null)
    }
    if (this.error) {
      return Promise.reject(this.error)
    }
    if (this.closed) {
      return Promise.resolve(null)
    }
    return new Promise<TerminalServerMessage | null>((resolve, reject) => {
      this.waiters.push({ resolve, reject })
    })
  }

  /** Closes the socket; the daemon ends the PTY. */
  close(): void {
    if (this.closed) {
      return
    }
    this.closed = true
    this.socket.close()
    for (const waiter of this.waiters.splice(0)) {
      waiter.resolve(null)
    }
  }

  async *[Symbol.asyncIterator](): AsyncIterator<TerminalServerMessage> {
    for (;;) {
      const message = await this.next()
      if (message === null) {
        return
      }
      yield message
    }
  }

  private send(message: TerminalClientMessage): void {
    if (this.closed || this.socket.readyState !== WebSocket.OPEN) {
      return
    }
    this.socket.send(JSON.stringify(message))
  }

  private push(message: TerminalServerMessage): void {
    const waiter = this.waiters.shift()
    if (waiter) {
      waiter.resolve(message)
    } else {
      this.queue.push(message)
    }
  }

  private finish(): void {
    if (this.closed) {
      return
    }
    this.closed = true
    for (const waiter of this.waiters.splice(0)) {
      waiter.resolve(null)
    }
  }

  private fail(error: unknown): void {
    if (this.closed || this.error) {
      return
    }
    this.error = error
    this.socket.close()
    for (const waiter of this.waiters.splice(0)) {
      waiter.reject(error)
    }
  }
}
