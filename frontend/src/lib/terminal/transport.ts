/** Size of a terminal in character cells. */
export interface TerminalSize {
  cols: number
  rows: number
}

/** What a transport reports once the remote shell is ready. */
export interface TerminalSessionInfo {
  program: string
  size: TerminalSize
  /** Whether the shell has native line editing and Tab completion. */
  lineEditing: boolean
}

export type TerminalStatus =
  | 'idle'
  | 'connecting'
  | 'open'
  | 'closed'
  | 'error'

/**
 * The only contract the terminal component needs.
 *
 * Implementations are free to use WebSocket, fetch, a local demo or a native
 * bridge. Keeping this interface transport-agnostic is what makes the component
 * reusable outside this project.
 */
export interface TerminalTransport {
  open(
    size: TerminalSize,
    handlers: TerminalTransportHandlers,
  ): Promise<TerminalSessionInfo>
  sendInput(data: Uint8Array): void
  resize(size: TerminalSize): void
  close(): void
}

export interface TerminalTransportHandlers {
  onData(data: Uint8Array): void
  onExit(code: number): void
  onError(error: Error): void
  onStatus?: (status: TerminalStatus) => void
}
