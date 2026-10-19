/** Messages the browser sends to `GET /api/terminal`. */
export type TerminalClientMessage =
  | { type: 'input'; data: string }
  | { type: 'resize'; cols: number; rows: number }
  | { type: 'ping' }

/** Messages the terminal endpoint sends to the browser. */
export type TerminalServerMessage =
  | {
      type: 'ready'
      shell: string
      cols: number
      rows: number
      line_editing: boolean
    }
  | { type: 'output'; data: string }
  | { type: 'exit'; code: number }
  | { type: 'error'; detail: string }
  | { type: 'pong' }

/** Encodes bytes without spreading a large array into a single call. */
export function encodeBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000
  let binary = ''

  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize))
  }

  return btoa(binary)
}

export function decodeBase64(text: string): Uint8Array {
  const binary = atob(text)
  const bytes = new Uint8Array(binary.length)

  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i)
  }

  return bytes
}

/** Validates an incoming value before it reaches the terminal. */
export function parseTerminalServerMessage(
  value: unknown,
): TerminalServerMessage {
  if (typeof value !== 'object' || value === null) {
    throw new Error('terminal message is not an object')
  }

  const message = value as Record<string, unknown>
  switch (message.type) {
    case 'ready':
      if (
        typeof message.shell !== 'string' ||
        typeof message.cols !== 'number' ||
        typeof message.rows !== 'number' ||
        (message.line_editing !== undefined &&
          typeof message.line_editing !== 'boolean')
      ) {
        throw new Error('invalid ready message')
      }
      return {
        type: 'ready',
        shell: message.shell,
        cols: message.cols,
        rows: message.rows,
        line_editing: message.line_editing === true,
      }
    case 'output':
      if (typeof message.data !== 'string') {
        throw new Error('invalid output message')
      }
      return { type: 'output', data: message.data }
    case 'exit':
      if (typeof message.code !== 'number') {
        throw new Error('invalid exit message')
      }
      return { type: 'exit', code: message.code }
    case 'error':
      if (typeof message.detail !== 'string') {
        throw new Error('invalid error message')
      }
      return { type: 'error', detail: message.detail }
    case 'pong':
      return { type: 'pong' }
    default:
      throw new Error(`unknown terminal message type: ${String(message.type)}`)
  }
}
