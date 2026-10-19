import { vi } from 'vitest'
import type {
  NetworkClient,
  NetworkHandlers,
  NetworkInterface,
  NetworkSnapshot,
} from '../lib/network/network-client'

export function networkInterface(
  overrides: Partial<NetworkInterface> = {},
): NetworkInterface {
  return {
    name: 'eth0',
    index: 2,
    state: 'up',
    up: true,
    running: true,
    loopback: false,
    mac: 'aa:bb:cc:dd:ee:ff',
    mtu: 1500,
    speed_mbps: 1000,
    duplex: 'full',
    addresses: [
      { family: 'ipv4', address: '192.0.2.10', prefix_len: 24 },
      { family: 'ipv6', address: '2001:db8::10', prefix_len: 64 },
    ],
    rx_bytes: 2048,
    tx_bytes: 1024,
    rx_packets: 10,
    tx_packets: 5,
    rx_errors: 0,
    tx_errors: 0,
    rx_dropped: 0,
    tx_dropped: 0,
    ...overrides,
  }
}

export function networkSnapshot(
  interfaces: NetworkInterface[] = [networkInterface()],
  overrides: Partial<NetworkSnapshot> = {},
): NetworkSnapshot {
  return {
    interfaces,
    timestamp_ms: 1_700_000_000_000,
    control_enabled: true,
    total: interfaces.length,
    truncated: false,
    ...overrides,
  }
}

export class FakeNetworkClient implements NetworkClient {
  current = networkSnapshot()
  listError?: Error
  stateError?: Error
  watchCalls: number[] = []
  stateCalls: Array<{ name: string; up: boolean }> = []
  handlers?: NetworkHandlers
  unsubscribes = 0

  list = vi.fn(async (): Promise<NetworkSnapshot> => {
    if (this.listError) throw this.listError
    return this.current
  })

  watch = vi.fn((intervalMs: number, handlers: NetworkHandlers) => {
    this.watchCalls.push(intervalMs)
    this.handlers = handlers
    handlers.onStatus?.('connecting')
    return () => {
      this.unsubscribes += 1
      handlers.onStatus?.('closed')
    }
  })

  setState = vi.fn(async (name: string, up: boolean): Promise<void> => {
    if (this.stateError) throw this.stateError
    this.stateCalls.push({ name, up })
  })

  emit(snapshot: NetworkSnapshot): void {
    this.current = snapshot
    this.handlers?.onSnapshot(snapshot)
  }

  status(status: 'live' | 'error' | 'closed'): void {
    this.handlers?.onStatus?.(status)
  }

  fail(error: Error): void {
    this.handlers?.onError?.(error)
  }
}
