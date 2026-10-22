import { beforeEach, describe, expect, it, vi } from 'vitest'
import { FakeWebSocket } from '../../test/fake-websocket'
import { encodeBase64 } from './protocol'
import type { TerminalTransportHandlers } from './transport'
import { WebSocketTerminalTransport } from './websocket-transport'

function handlers(): TerminalTransportHandlers {
  return {
    onData: vi.fn(),
    onExit: vi.fn(),
    onError: vi.fn(),
    onStatus: vi.fn(),
  }
}

async function openWith(
  transport: WebSocketTerminalTransport,
  size = { cols: 100, rows: 30 },
): Promise<TerminalTransportHandlers> {
  const h = handlers()
  const opening = transport.open(size, h)
  const socket = FakeWebSocket.instances[0]
  socket?.open()
  socket?.message(
    JSON.stringify({
      type: 'ready',
      shell: '/bin/bash',
      cols: size.cols,
      rows: size.rows,
      line_editing: true,
    }),
  )
  await opening
  return h
}

describe('WebSocketTerminalTransport', () => {
  beforeEach(() => {
    FakeWebSocket.reset()
    vi.stubGlobal('WebSocket', FakeWebSocket)
  })

  it('resolves ready and forwards line editing', async () => {
    const transport = new WebSocketTerminalTransport({
      url: (size) =>
        `ws://example.test/terminal?cols=${size.cols}&rows=${size.rows}`,
    })

    const h = handlers()
    const opening = transport.open({ cols: 120, rows: 40 }, h)
    const socket = FakeWebSocket.instances[0]
    expect(socket?.url).toBe(
      'ws://example.test/terminal?cols=120&rows=40',
    )
    socket?.open()
    socket?.message(
      JSON.stringify({
        type: 'ready',
        shell: '/bin/bash',
        cols: 120,
        rows: 40,
        line_editing: true,
      }),
    )

    await expect(opening).resolves.toEqual({
      program: '/bin/bash',
      size: { cols: 120, rows: 40 },
      lineEditing: true,
    })
    expect(h.onStatus).toHaveBeenCalledWith('open')
  })

  it('treats a missing line_editing as false', async () => {
    const transport = new WebSocketTerminalTransport({
      url: 'ws://example.test/terminal',
    })
    const h = handlers()
    const opening = transport.open({ cols: 80, rows: 24 }, h)
    const socket = FakeWebSocket.instances[0]
    socket?.open()
    socket?.message(
      JSON.stringify({
        type: 'ready',
        shell: '/bin/sh',
        cols: 80,
        rows: 24,
      }),
    )

    await expect(opening).resolves.toMatchObject({ lineEditing: false })
  })

  it('encodes input, chunks large pastes and sends resize', async () => {
    const transport = new WebSocketTerminalTransport({
      url: 'ws://example.test/terminal',
    })
    await openWith(transport)
    const socket = FakeWebSocket.instances[0]

    transport.sendInput(new Uint8Array([0x68, 0x69]))
    expect(JSON.parse(socket?.sent[0] ?? '{}')).toEqual({
      type: 'input',
      data: encodeBase64(new Uint8Array([0x68, 0x69])),
    })

    transport.sendInput(new Uint8Array(20 * 1024))
    expect(socket?.sent).toHaveLength(3)

    transport.resize({ cols: 90, rows: 25 })
    expect(JSON.parse(socket?.sent[3] ?? '{}')).toEqual({
      type: 'resize',
      cols: 90,
      rows: 25,
    })
  })

  it('decodes output and reports exit', async () => {
    const transport = new WebSocketTerminalTransport({
      url: 'ws://example.test/terminal',
    })
    const h = await openWith(transport)
    const socket = FakeWebSocket.instances[0]

    socket?.message(
      JSON.stringify({ type: 'output', data: encodeBase64(new TextEncoder().encode('hi')) }),
    )
    expect(h.onData).toHaveBeenCalledWith(new TextEncoder().encode('hi'))

    socket?.message(JSON.stringify({ type: 'exit', code: 3 }))
    expect(h.onExit).toHaveBeenCalledWith(3)
    expect(h.onStatus).toHaveBeenLastCalledWith('closed')
  })

  it('reports server errors', async () => {
    const transport = new WebSocketTerminalTransport({
      url: 'ws://example.test/terminal',
    })
    const h = await openWith(transport)
    FakeWebSocket.instances[0]?.message(
      JSON.stringify({ type: 'error', detail: 'session cap reached' }),
    )
    expect(h.onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'session cap reached' }),
    )
    expect(h.onStatus).toHaveBeenLastCalledWith('error')
  })

  it('rejects open when the socket closes before ready', async () => {
    const transport = new WebSocketTerminalTransport({
      url: 'ws://example.test/terminal',
    })
    const opening = transport.open({ cols: 80, rows: 24 }, handlers())
    const socket = FakeWebSocket.instances[0]
    socket?.close(1006)

    await expect(opening).rejects.toThrow('closed before opening')
  })
})
