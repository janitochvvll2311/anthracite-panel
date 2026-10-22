import { describe, expect, it } from 'vitest'
import {
  addressSummary,
  compareNetworkRows,
  computeNetworkRows,
  filterNetworkRows,
  formatAddress,
  formatBytes,
  formatRate,
  formatSpeed,
  interfaceState,
} from './display'
import { networkInterface, networkSnapshot } from '../../test/fake-network-client'

describe('network display helpers', () => {
  it('formats byte counts with binary units', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(1023)).toBe('1023 B')
    expect(formatBytes(1024)).toBe('1.0 KiB')
    expect(formatBytes(1024 * 1024 * 1.5)).toBe('1.5 MiB')
  })

  it('formats rates and unknown samples', () => {
    expect(formatRate(null)).toBe('—')
    expect(formatRate(0)).toBe('0 B/s')
    expect(formatRate(2048)).toBe('2.0 KiB/s')
    expect(formatSpeed(null)).toBe('—')
    expect(formatSpeed(1000)).toBe('1000 Mb/s')
  })

  it('summarises addresses and state', () => {
    const iface = networkInterface({
      addresses: [
        { family: 'ipv4', address: '192.0.2.10', prefix_len: 24 },
        { family: 'ipv6', address: '2001:db8::10', prefix_len: 64 },
      ],
    })

    expect(formatAddress(iface.addresses[0]!)).toBe('192.0.2.10/24')
    expect(addressSummary(iface)).toBe('192.0.2.10/24 +1')
    expect(interfaceState(iface)).toEqual({ label: 'up', tone: 'up' })
    expect(interfaceState(networkInterface({ up: false }))).toEqual({
      label: 'down',
      tone: 'down',
    })
    expect(
      interfaceState(
        networkInterface({ up: true, running: false, state: 'unknown' }),
      ),
    ).toEqual({ label: 'up', tone: 'up' })
    expect(
      interfaceState(
        networkInterface({ up: true, running: false, state: 'down' }),
      ),
    ).toEqual({ label: 'no carrier', tone: 'warn' })
    expect(
      interfaceState(networkInterface({ up: true, running: false })),
    ).toEqual({ label: 'up', tone: 'up' })
    expect(
      addressSummary(
        networkInterface({
          addresses: [],
          mac: 'aa:bb:cc:dd:ee:ff',
        }),
      ),
    ).toBe('aa:bb:cc:dd:ee:ff')
  })

  it('computes rates from consecutive snapshots', () => {
    const first = networkSnapshot()
    expect(computeNetworkRows(first)[0]?.rx_rate).toBeNull()

    const second = networkSnapshot(
      [networkInterface({ rx_bytes: 4096, tx_bytes: 2048 })],
      { timestamp_ms: first.timestamp_ms + 2000 },
    )
    const rows = computeNetworkRows(second, first)
    expect(rows[0]?.rx_rate).toBe(1024)
    expect(rows[0]?.tx_rate).toBe(512)
  })

  it('does not invent rates after a counter reset or a zero interval', () => {
    const first = networkSnapshot()
    const reset = networkSnapshot(
      [networkInterface({ rx_bytes: 1, tx_bytes: 1 })],
      { timestamp_ms: first.timestamp_ms + 2000 },
    )
    const rows = computeNetworkRows(reset, first)
    expect(rows[0]?.rx_rate).toBeNull()
    expect(rows[0]?.tx_rate).toBeNull()

    const sameTime = networkSnapshot(
      [networkInterface({ rx_bytes: 4096, tx_bytes: 2048 })],
      { timestamp_ms: first.timestamp_ms },
    )
    expect(computeNetworkRows(sameTime, first)[0]?.rx_rate).toBeNull()
  })

  it('filters by name, address, MAC and state', () => {
    const rows = computeNetworkRows(
      networkSnapshot([
        networkInterface(),
        networkInterface({
          name: 'lo',
          index: 1,
          state: 'unknown',
          mac: '00:00:00:00:00:00',
          addresses: [{ family: 'ipv4', address: '127.0.0.1', prefix_len: 8 }],
        }),
      ]),
    )

    expect(filterNetworkRows(rows, 'eth0').map((row) => row.name)).toEqual([
      'eth0',
    ])
    expect(filterNetworkRows(rows, '127.0.0.1').map((row) => row.name)).toEqual([
      'lo',
    ])
    expect(filterNetworkRows(rows, 'aa:bb').map((row) => row.name)).toEqual([
      'eth0',
    ])
    expect(filterNetworkRows(rows, 'unknown').map((row) => row.name)).toEqual([
      'lo',
    ])
    expect(filterNetworkRows(rows, '')).toHaveLength(2)
  })

  it('sorts numbers and strings with nulls last', () => {
    const low = computeNetworkRows(
      networkSnapshot([networkInterface({ name: 'b', rx_bytes: 10 })]),
    )[0]!
    const high = computeNetworkRows(
      networkSnapshot([networkInterface({ name: 'a', rx_bytes: 20 })]),
    )[0]!
    const missing = { ...low, name: 'c', rx_rate: null }

    const rows = [low, high, missing].sort((left, right) =>
      compareNetworkRows(left, right, 'rx_bytes', 'desc'),
    )
    expect(rows.map((row) => row.name)).toEqual(['a', 'b', 'c'])

    const byName = [low, high].sort((left, right) =>
      compareNetworkRows(left, right, 'name', 'asc'),
    )
    expect(byName.map((row) => row.name)).toEqual(['a', 'b'])
  })
})
