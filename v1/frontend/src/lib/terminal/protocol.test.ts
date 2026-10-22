import { describe, expect, it } from 'vitest'
import {
  decodeBase64,
  encodeBase64,
  parseTerminalServerMessage,
} from './protocol'

describe('base64 helpers', () => {
  it('round-trips arbitrary bytes, including non-UTF-8 values', () => {
    const bytes = new Uint8Array([0x00, 0x0a, 0x0d, 0xff, 0xfe, 0x80])
    expect(decodeBase64(encodeBase64(bytes))).toEqual(bytes)
  })

  it('round-trips an empty payload', () => {
    expect(encodeBase64(new Uint8Array())).toBe('')
    expect(decodeBase64('')).toEqual(new Uint8Array())
  })

  it('encodes a payload larger than the chunk boundary', () => {
    const bytes = new Uint8Array(0x8000 + 3)
    for (let i = 0; i < bytes.length; i++) bytes[i] = i % 256
    expect(decodeBase64(encodeBase64(bytes))).toEqual(bytes)
  })
})

describe('parseTerminalServerMessage', () => {
  it('parses a ready frame with line editing', () => {
    expect(
      parseTerminalServerMessage({
        type: 'ready',
        shell: '/bin/bash',
        cols: 120,
        rows: 40,
        line_editing: true,
      }),
    ).toEqual({
      type: 'ready',
      shell: '/bin/bash',
      cols: 120,
      rows: 40,
      line_editing: true,
    })
  })

  it('treats an omitted line_editing as false for old daemons', () => {
    expect(
      parseTerminalServerMessage({
        type: 'ready',
        shell: '/bin/sh',
        cols: 80,
        rows: 24,
      }),
    ).toMatchObject({ type: 'ready', line_editing: false })
  })

  it('parses output, exit, error and pong frames', () => {
    expect(
      parseTerminalServerMessage({ type: 'output', data: 'aGk=' }),
    ).toEqual({ type: 'output', data: 'aGk=' })
    expect(parseTerminalServerMessage({ type: 'exit', code: 7 })).toEqual({
      type: 'exit',
      code: 7,
    })
    expect(
      parseTerminalServerMessage({ type: 'error', detail: 'nope' }),
    ).toEqual({ type: 'error', detail: 'nope' })
    expect(parseTerminalServerMessage({ type: 'pong' })).toEqual({
      type: 'pong',
    })
  })

  it('rejects malformed messages', () => {
    expect(() => parseTerminalServerMessage(null)).toThrow('not an object')
    expect(() => parseTerminalServerMessage({ type: 'ready' })).toThrow(
      'invalid ready',
    )
    expect(() =>
      parseTerminalServerMessage({
        type: 'ready',
        shell: '/bin/sh',
        cols: 80,
        rows: 24,
        line_editing: 'yes',
      }),
    ).toThrow('invalid ready')
    expect(() => parseTerminalServerMessage({ type: 'output' })).toThrow(
      'invalid output',
    )
    expect(() => parseTerminalServerMessage({ type: 'exit' })).toThrow(
      'invalid exit',
    )
    expect(() => parseTerminalServerMessage({ type: 'error' })).toThrow(
      'invalid error',
    )
    expect(() => parseTerminalServerMessage({ type: 'nope' })).toThrow(
      'unknown terminal message type',
    )
  })
})
