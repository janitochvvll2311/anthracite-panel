import { expect, test } from 'vitest'

import {
  computeRows,
  countUp,
  filterRows,
  formatAddress,
  formatRate,
  formatSpeed,
  hasErrors,
} from './lib/network/display'
import type { NetworkInterface, NetworkSnapshot } from 'anth-wire'

function iface(overrides: Partial<NetworkInterface> = {}): NetworkInterface {
  return {
    name: 'eth0',
    index: 2,
    kind: 'ether',
    mac: '00:11:22:33:44:55',
    mtu: 1500,
    state: 'up',
    up: true,
    running: true,
    loopback: false,
    physical: false,
    addresses: [
      { family: 'inet', address: '192.168.1.10', prefix: 24, scope: 'global' },
    ],
    rx_bytes: 1000,
    tx_bytes: 500,
    rx_packets: 10,
    tx_packets: 5,
    rx_errors: 0,
    tx_errors: 0,
    rx_dropped: 0,
    tx_dropped: 0,
    speed_mbps: 1000,
    ...overrides,
  }
}

function snapshot(overrides: Partial<NetworkSnapshot> = {}): NetworkSnapshot {
  return {
    interfaces: [iface()],
    routes: [{ family: 'inet', gateway: '192.168.1.1', interface: 'eth0' }],
    timestamp_ms: 1_000,
    control_enabled: true,
    manage_enabled: false,
    ...overrides,
  }
}

test('rates need two snapshots and tolerate a counter reset', () => {
  const first = computeRows(snapshot())
  expect(first[0].rx_rate).toBeNull()
  expect(first[0].tx_rate).toBeNull()

  const second = computeRows(
    snapshot({
      timestamp_ms: 3_000,
      interfaces: [iface({ rx_bytes: 5_096, tx_bytes: 1_524 })],
    }),
    snapshot(),
  )
  expect(second[0].rx_rate).toBe(2_048)
  expect(second[0].tx_rate).toBe(512)

  const reset = computeRows(
    snapshot({
      timestamp_ms: 4_000,
      interfaces: [iface({ rx_bytes: 10, tx_bytes: 10 })],
    }),
    snapshot({ timestamp_ms: 3_000 }),
  )
  expect(reset[0].rx_rate).toBeNull()
  expect(reset[0].tx_rate).toBeNull()

  const sameClock = computeRows(snapshot(), snapshot())
  expect(sameClock[0].rx_rate).toBeNull()

  const appeared = computeRows(
    snapshot({
      timestamp_ms: 3_000,
      interfaces: [iface({ name: 'wlan0' })],
    }),
    snapshot(),
  )
  expect(appeared[0].rx_rate).toBeNull()
})

test('rates and speeds use the byte and bit units the panel already uses', () => {
  expect(formatRate(null)).toBe('—')
  expect(formatRate(Number.NaN)).toBe('—')
  expect(formatRate(0)).toBe('0 B/s')
  expect(formatRate(1_536)).toBe('1.5 KiB/s')
  expect(formatRate(2 * 1024 * 1024)).toBe('2.0 MiB/s')

  expect(formatSpeed(null)).toBe('—')
  expect(formatSpeed(0)).toBe('—')
  expect(formatSpeed(100)).toBe('100 Mbps')
  expect(formatSpeed(1000)).toBe('1 Gbps')
  expect(formatSpeed(2500)).toBe('2.5 Gbps')
})

test('addresses and errors are formatted for display', () => {
  expect(
    formatAddress({ family: 'inet6', address: 'fe80::1', prefix: 64, scope: 'link' }),
  ).toBe('fe80::1/64')
  expect(hasErrors(iface())).toBe(false)
  expect(hasErrors(iface({ rx_dropped: 3 }))).toBe(true)
  expect(hasErrors(iface({ tx_errors: 1 }))).toBe(true)
})

test('the filter matches names, kinds, states, MACs and addresses', () => {
  const rows = computeRows(
    snapshot({
      interfaces: [
        iface(),
        iface({
          name: 'lo',
          index: 1,
          kind: 'loopback',
          loopback: true,
          mac: '',
          addresses: [
            { family: 'inet', address: '127.0.0.1', prefix: 8, scope: 'host' },
          ],
        }),
      ],
    }),
  )
  expect(countUp(rows)).toBe(2)
  expect(filterRows(rows, '')).toHaveLength(2)
  expect(filterRows(rows, 'ETH')).toHaveLength(1)
  expect(filterRows(rows, '192.168.1')).toHaveLength(1)
  expect(filterRows(rows, '00:11:22')).toHaveLength(1)
  expect(filterRows(rows, 'loopback').map((row) => row.name)).toEqual(['lo'])
  expect(filterRows(rows, 'nothing')).toHaveLength(0)
})
