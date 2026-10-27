/**
 * Derived network values: per-interface rates, display strings and filtering.
 *
 * The daemon reports cumulative counters. A rate needs two snapshots and their
 * timestamps, so it is computed here and never guessed by the daemon.
 */

import { formatBytes } from '../../format'
import type {
  NetworkAddress,
  NetworkInterface,
  NetworkSnapshot,
} from '../../network-client'

export interface NetworkRow extends NetworkInterface {
  /** Bytes per second since the previous snapshot, or `null` without one. */
  rx_rate: number | null
  tx_rate: number | null
}

/** Pairs each interface with the previous snapshot to derive its rates. */
export function computeRows(
  snapshot: NetworkSnapshot,
  previous?: NetworkSnapshot,
): NetworkRow[] {
  const seconds = previous
    ? (snapshot.timestamp_ms - previous.timestamp_ms) / 1000
    : 0
  const before = new Map(
    (previous?.interfaces ?? []).map((iface) => [iface.name, iface]),
  )
  const rate = (now: number, then: number | undefined): number | null => {
    if (then === undefined || seconds <= 0 || now < then) return null
    return (now - then) / seconds
  }
  return snapshot.interfaces.map((iface) => {
    const last = before.get(iface.name)
    return {
      ...iface,
      rx_rate: rate(iface.rx_bytes, last?.rx_bytes),
      tx_rate: rate(iface.tx_bytes, last?.tx_bytes),
    }
  })
}

/** A rate as `1.2 MiB/s`, or an em dash until two snapshots exist. */
export function formatRate(rate: number | null): string {
  if (rate === null || !Number.isFinite(rate)) return '—'
  return `${formatBytes(rate)}/s`
}

/** A link speed as `100 Mbps`, `2.5 Gbps` or an em dash. */
export function formatSpeed(mbps: number | null | undefined): string {
  if (mbps === null || mbps === undefined || mbps <= 0) return '—'
  if (mbps < 1000) return `${mbps} Mbps`
  const gbps = mbps / 1000
  return `${Number.isInteger(gbps) ? gbps : gbps.toFixed(1)} Gbps`
}

/** One address in `address/prefix` form. */
export function formatAddress(address: NetworkAddress): string {
  return `${address.address}/${address.prefix}`
}

/** Whether an interface has anything the operator should look at. */
export function hasErrors(iface: NetworkInterface): boolean {
  return iface.rx_errors + iface.tx_errors + iface.rx_dropped + iface.tx_dropped > 0
}

/** Filters by name, kind, state, MAC or address. */
export function filterRows(rows: NetworkRow[], query: string): NetworkRow[] {
  const needle = query.trim().toLowerCase()
  if (needle === '') return rows
  return rows.filter((iface) => {
    const haystack = [
      iface.name,
      iface.kind,
      iface.state,
      iface.mac,
      ...iface.addresses.map(formatAddress),
    ]
      .join(' ')
      .toLowerCase()
    return haystack.includes(needle)
  })
}

/** How many interfaces are administratively up. */
export function countUp(rows: NetworkRow[]): number {
  return rows.filter((iface) => iface.up).length
}
