import type {
  NetworkAddress,
  NetworkInterface,
  NetworkRow,
  NetworkSnapshot,
} from './network-client'
import { formatBytes } from '../format'

export type NetworkSortKey =
  | 'name'
  | 'state'
  | 'speed'
  | 'rx_rate'
  | 'tx_rate'
  | 'rx_bytes'
  | 'tx_bytes'

export type NetworkSortDirection = 'asc' | 'desc'

export { formatBytes } from '../format'

/** Formats a per-second rate; `null` means there is no prior sample yet. */
export function formatRate(bytesPerSecond: number | null): string {
  if (bytesPerSecond === null) return '—'
  return `${formatBytes(bytesPerSecond)}/s`
}

export function formatSpeed(speedMbps: number | null): string {
  return speedMbps === null ? '—' : `${speedMbps} Mb/s`
}

export function formatAddress(address: NetworkAddress): string {
  return `${address.address}/${address.prefix_len}`
}

/** A compact address cell: the first address plus a count of the rest. */
export function addressSummary(iface: NetworkInterface): string {
  if (iface.addresses.length === 0) return iface.mac || '—'
  const [first, ...rest] = iface.addresses
  return rest.length === 0 ? formatAddress(first!) : `${formatAddress(first!)} +${rest.length}`
}

export interface InterfaceState {
  label: string
  tone: 'up' | 'down' | 'warn'
}

export function interfaceState(iface: NetworkInterface): InterfaceState {
  if (!iface.up) return { label: 'down', tone: 'down' }
  if (iface.state === 'down') return { label: 'no carrier', tone: 'warn' }
  if (iface.state === 'up' || iface.state === 'unknown' || iface.running) {
    return { label: 'up', tone: 'up' }
  }
  return { label: iface.state, tone: 'warn' }
}

/**
 * Adds RX/TX rates from the previous snapshot. Counter resets and missing
 * samples produce `null`, which the table renders as an em dash.
 */
export function computeNetworkRows(
  current: NetworkSnapshot,
  previous?: NetworkSnapshot,
): NetworkRow[] {
  const previousByName = new Map(
    (previous?.interfaces ?? []).map((iface) => [iface.name, iface]),
  )
  const elapsed =
    previous === undefined
      ? 0
      : Math.max(0, (current.timestamp_ms - previous.timestamp_ms) / 1000)

  return current.interfaces.map((iface) => {
    const before = previousByName.get(iface.name)
    const rate = (
      now: number,
      then: number | undefined,
    ): number | null => {
      if (then === undefined || elapsed <= 0 || now < then) return null
      return (now - then) / elapsed
    }

    return {
      ...iface,
      rx_rate: rate(iface.rx_bytes, before?.rx_bytes),
      tx_rate: rate(iface.tx_bytes, before?.tx_bytes),
    }
  })
}

export function filterNetworkRows(rows: NetworkRow[], filter: string): NetworkRow[] {
  const needle = filter.trim().toLowerCase()
  if (needle === '') return rows

  return rows.filter((iface) =>
    [
      iface.name,
      iface.state,
      iface.mac,
      String(iface.index),
      ...iface.addresses.map(formatAddress),
    ].some((value) => value.toLowerCase().includes(needle)),
  )
}

export function totalErrors(iface: NetworkInterface): number {
  return iface.rx_errors + iface.tx_errors + iface.rx_dropped + iface.tx_dropped
}

function sortValue(row: NetworkRow, key: NetworkSortKey): string | number | null {
  switch (key) {
    case 'name':
      return row.name
    case 'state':
      return interfaceState(row).label
    case 'speed':
      return row.speed_mbps
    case 'rx_rate':
      return row.rx_rate
    case 'tx_rate':
      return row.tx_rate
    case 'rx_bytes':
      return row.rx_bytes
    case 'tx_bytes':
      return row.tx_bytes
  }
}

export function compareNetworkRows(
  left: NetworkRow,
  right: NetworkRow,
  key: NetworkSortKey,
  direction: NetworkSortDirection,
): number {
  const a = sortValue(left, key)
  const b = sortValue(right, key)
  const order = direction === 'asc' ? 1 : -1

  if (a === null && b === null) return left.name.localeCompare(right.name)
  if (a === null) return 1
  if (b === null) return -1
  if (typeof a === 'string' && typeof b === 'string') {
    return a.localeCompare(b) * order
  }
  return (Number(a) - Number(b)) * order
}
