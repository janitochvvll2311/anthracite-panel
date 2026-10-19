import { beforeEach, describe, expect, it, vi } from 'vitest'
import { FakeWebSocket } from '../../test/fake-websocket'
import { JsonWebSocketTransport } from './json-websocket'

describe('JsonWebSocketTransport', () => {
  beforeEach(() => {
    FakeWebSocket.reset()
    vi.stubGlobal('WebSocket', FakeWebSocket)
  })

  it('connects, sends JSON and receives parsed messages', async () => {
    const messages: Array<{ value: string }> = []
    const transport = new JsonWebSocketTransport<
      { value: string },
      { value: string }
    >({
      url: 'ws://example.test/socket',
      onMessage: (message) => messages.push(message),
    })

    const connecting = transport.connect()
    const socket = FakeWebSocket.instances[0]
    expect(socket?.url).toBe('ws://example.test/socket')
    expect(transport.state).toBe('connecting')

    socket?.open()
    await connecting
    expect(transport.state).toBe('open')

    transport.send({ value: 'out' })
    expect(socket?.sent).toEqual(['{"value":"out"}'])

    socket?.message('{"value":"in"}')
    expect(messages).toEqual([{ value: 'in' }])
  })

  it('uses a custom deserializer and reports parse failures', async () => {
    const onMessage = vi.fn()
    const onMessageError = vi.fn()
    const transport = new JsonWebSocketTransport<never, { raw: string }>({
      url: 'ws://example.test/socket',
      deserialize: (data) => ({ raw: data.toUpperCase() }),
      onMessage,
      onMessageError,
    })

    const connecting = transport.connect()
    const socket = FakeWebSocket.instances[0]
    socket?.open()
    await connecting

    socket?.message('hello')
    expect(onMessage).toHaveBeenCalledWith({ raw: 'HELLO' })

    const failing = new JsonWebSocketTransport({
      url: 'ws://example.test/socket',
      deserialize: () => {
        throw new Error('bad frame')
      },
      onMessageError,
    })
    const failingConnect = failing.connect()
    const failingSocket = FakeWebSocket.instances[1]
    failingSocket?.open()
    await failingConnect
    failingSocket?.message('anything')
    expect(onMessageError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'bad frame' }),
    )
  })

  it('rejects non-text frames and malformed JSON', async () => {
    const onMessageError = vi.fn()
    const transport = new JsonWebSocketTransport({
      url: 'ws://example.test/socket',
      onMessageError,
    })

    const connecting = transport.connect()
    const socket = FakeWebSocket.instances[0]
    socket?.open()
    await connecting

    socket?.message(new ArrayBuffer(2))
    expect(onMessageError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'expected a text WebSocket frame' }),
    )

    socket?.message('not json')
    expect(onMessageError).toHaveBeenLastCalledWith(expect.any(Error))
  })

  it('rejects a connect that closes before opening', async () => {
    const transport = new JsonWebSocketTransport({
      url: 'ws://example.test/socket',
    })

    const connecting = transport.connect()
    const socket = FakeWebSocket.instances[0]
    socket?.close(1006, 'network')

    await expect(connecting).rejects.toThrow(
      'WebSocket closed before opening (code 1006)',
    )
    expect(transport.state).toBe('closed')
  })

  it('transitions through closing to closed on close()', async () => {
    const onClose = vi.fn()
    const transport = new JsonWebSocketTransport({
      url: 'ws://example.test/socket',
      onClose,
    })

    const connecting = transport.connect()
    const socket = FakeWebSocket.instances[0]
    socket?.open()
    await connecting

    transport.close()
    expect(transport.state).toBe('closed')
    expect(onClose).toHaveBeenCalledWith(
      expect.objectContaining({ code: 1000 }),
    )
  })

  it('throws when sending on a closed socket', () => {
    const transport = new JsonWebSocketTransport({
      url: 'ws://example.test/socket',
    })
    expect(() => transport.send({ value: 'x' })).toThrow('not open')
  })
})
