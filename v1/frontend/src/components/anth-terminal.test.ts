import { afterEach, describe, expect, it, vi } from 'vitest'
import { AnthTerminal } from './anth-terminal'
import type {
  TerminalSessionInfo,
  TerminalSize,
  TerminalTransport,
  TerminalTransportHandlers,
} from '../lib/terminal/transport'

const xterm = vi.hoisted(() => {
  class FakeBuffer {
    active: { type: 'normal' | 'alternate' } = { type: 'normal' }
    private listener?: (buffer: { type: 'normal' | 'alternate' }) => void

    onBufferChange(listener: (buffer: { type: 'normal' | 'alternate' }) => void) {
      this.listener = listener
      return { dispose() {} }
    }

    setType(type: 'normal' | 'alternate'): void {
      this.active.type = type
      this.listener?.(this.active)
    }
  }

  class FakeTerminal {
    static instances: FakeTerminal[] = []

    cols = 80
    rows = 24
    options: Record<string, unknown> = {}
    buffer = new FakeBuffer()
    modes = { applicationCursorKeysMode: false }
    writes: Array<string | Uint8Array> = []

    private dataHandler?: (data: string) => void
    private resizeHandler?: (size: TerminalSize) => void
    private keyHandler?: (event: KeyboardEvent) => boolean

    constructor(options: Record<string, unknown>) {
      Object.assign(this.options, options)
      FakeTerminal.instances.push(this)
    }

    loadAddon(): void {}
    open(): void {}
    focus(): void {}
    blur(): void {}
    clear(): void {}
    reset(): void {}
    dispose(): void {}

    onData(handler: (data: string) => void) {
      this.dataHandler = handler
      return { dispose() {} }
    }

    onResize(handler: (size: TerminalSize) => void) {
      this.resizeHandler = handler
      return { dispose() {} }
    }

    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean): void {
      this.keyHandler = handler
    }

    write(data: string | Uint8Array): void {
      this.writes.push(data)
    }

    emit(data: string): void {
      this.dataHandler?.(data)
    }

    press(key: string, init: KeyboardEventInit = {}): boolean | undefined {
      return this.keyHandler?.(
        new KeyboardEvent('keydown', { key, ...init }),
      )
    }

    resize(cols: number, rows: number): void {
      this.cols = cols
      this.rows = rows
      this.resizeHandler?.({ cols, rows })
    }
  }

  class FakeFitAddon {
    activate(): void {}
    dispose(): void {}
    fit(): void {}
    proposeDimensions(): undefined {
      return undefined
    }
  }

  return { FakeTerminal, FakeFitAddon }
})

vi.mock('@xterm/xterm', () => ({ Terminal: xterm.FakeTerminal }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: xterm.FakeFitAddon }))
vi.mock('@xterm/xterm/css/xterm.css?inline', () => ({ default: '' }))

function createTransport(lineEditing: boolean) {
  const handlers = { current: undefined as TerminalTransportHandlers | undefined }
  const open = vi.fn(
    async (
      size: TerminalSize,
      callbacks: TerminalTransportHandlers,
    ): Promise<TerminalSessionInfo> => {
      handlers.current = callbacks
      callbacks.onStatus?.('open')
      return {
        program: lineEditing ? '/bin/bash' : '/bin/sh',
        size,
        lineEditing,
      }
    },
  )
  const sendInput = vi.fn()
  const resize = vi.fn()
  const close = vi.fn()

  const transport: TerminalTransport = { open, sendInput, resize, close }
  return { transport, open, sendInput, resize, close, handlers }
}

async function settle(element: AnthTerminal): Promise<void> {
  await element.updateComplete
  for (let i = 0; i < 20 && element.status !== 'open'; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  lineEditing: boolean,
  keyboardMode?: 'auto' | 'shell' | 'local',
) {
  const transport = createTransport(lineEditing)
  const element = document.createElement('anth-terminal') as AnthTerminal
  element.transport = transport.transport
  if (keyboardMode) element.keyboardMode = keyboardMode
  document.body.appendChild(element)
  await settle(element)

  const terminal = xterm.FakeTerminal.instances.at(-1)
  if (!terminal) throw new Error('fake xterm was not created')
  return { element, terminal, ...transport }
}

afterEach(() => {
  xterm.FakeTerminal.instances.length = 0
  document.body.innerHTML = ''
})

describe('AnthTerminal keyboard modes', () => {
  it('uses local history and Tab completion when the shell has no line editing', async () => {
    const { element, terminal, sendInput } = await mount(false)
    expect(element.effectiveKeyboardMode).toBe('local')
    expect(element.session).toMatchObject({
      program: '/bin/sh',
      lineEditing: false,
    })

    terminal.emit('echo alpha\r')
    terminal.emit('echo beta\r')
    terminal.emit('echo al')
    sendInput.mockClear()

    expect(terminal.press('Tab')).toBe(false)
    expect(sendInput).toHaveBeenLastCalledWith(
      new TextEncoder().encode('\u0015echo alpha'),
    )

    terminal.emit('\u007f')
    terminal.emit('\u007f')
    terminal.emit('\u007f')
    sendInput.mockClear()
    expect(terminal.press('ArrowUp')).toBe(false)
    expect(sendInput).toHaveBeenLastCalledWith(
      new TextEncoder().encode('\u0015echo beta'),
    )
  })

  it('cycles through multiple Tab matches', async () => {
    const { terminal, sendInput } = await mount(false)
    terminal.emit('echo alpha\r')
    terminal.emit('echo beta\r')
    terminal.emit('echo ')
    sendInput.mockClear()

    expect(terminal.press('Tab')).toBe(false)
    expect(sendInput).toHaveBeenLastCalledWith(
      new TextEncoder().encode('\u0015echo beta'),
    )

    expect(terminal.press('Tab')).toBe(false)
    expect(sendInput).toHaveBeenLastCalledWith(
      new TextEncoder().encode('\u0015echo alpha'),
    )
  })

  it('delegates arrows and Tab to a line-editing shell', async () => {
    const { element, terminal, sendInput } = await mount(true)
    expect(element.effectiveKeyboardMode).toBe('shell')

    sendInput.mockClear()
    expect(terminal.press('ArrowUp')).toBe(true)
    expect(terminal.press('Tab')).toBe(true)
    expect(sendInput).not.toHaveBeenCalled()
  })

  it('honours explicit shell and local overrides', async () => {
    const forcedShell = await mount(false, 'shell')
    expect(forcedShell.element.effectiveKeyboardMode).toBe('shell')
    expect(forcedShell.terminal.press('Tab')).toBe(true)
    expect(forcedShell.sendInput).not.toHaveBeenCalled()

    xterm.FakeTerminal.instances.length = 0
    document.body.innerHTML = ''

    const forcedLocal = await mount(true, 'local')
    expect(forcedLocal.element.effectiveKeyboardMode).toBe('local')
    forcedLocal.terminal.emit('echo first\r')
    forcedLocal.terminal.emit('echo f')
    forcedLocal.sendInput.mockClear()
    expect(forcedLocal.terminal.press('Tab')).toBe(false)
    expect(forcedLocal.sendInput).toHaveBeenLastCalledWith(
      new TextEncoder().encode('\u0015echo first'),
    )
  })

  it('leaves the alternate buffer to full-screen programs', async () => {
    const { terminal, sendInput } = await mount(false)
    terminal.emit('echo top\r')
    terminal.buffer.setType('alternate')
    sendInput.mockClear()

    expect(terminal.press('ArrowUp')).toBe(true)
    expect(terminal.press('Tab')).toBe(true)
    expect(sendInput).not.toHaveBeenCalled()
  })
})

describe('AnthTerminal lifecycle', () => {
  it('does not create a second xterm instance on reconnect', async () => {
    const { element } = await mount(false)
    expect(xterm.FakeTerminal.instances).toHaveLength(1)

    await element.connect()
    expect(xterm.FakeTerminal.instances).toHaveLength(1)
  })

  it('writes local output and dispatches ready details', async () => {
    const transport = createTransport(true)
    const element = document.createElement('anth-terminal') as AnthTerminal
    const ready = vi.fn()
    element.addEventListener('anth-terminal-ready', ready)
    element.transport = transport.transport
    document.body.appendChild(element)
    await settle(element)

    expect(ready).toHaveBeenCalledWith(
      expect.objectContaining({
        detail: {
          program: '/bin/bash',
          size: { cols: 80, rows: 24 },
          lineEditing: true,
        },
      }),
    )

    const terminal = xterm.FakeTerminal.instances.at(-1)
    element.write('local')
    expect(terminal?.writes).toContain('local')
  })
})
