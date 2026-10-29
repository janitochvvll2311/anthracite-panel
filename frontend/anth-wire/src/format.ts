const BYTE_UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB']

export function formatBytes(bytes: number): string {
  let value = Math.max(0, Number.isFinite(bytes) ? bytes : 0)
  let unit = 0
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024
    unit += 1
  }
  if (unit === 0) return `${Math.round(value)} B`
  return `${value.toFixed(1)} ${BYTE_UNITS[unit]}`
}

export function formatKib(kib: number): string {
  const value = Math.max(0, Number.isFinite(kib) ? kib : 0)
  const mib = value / 1024
  if (mib >= 1024) return `${(mib / 1024).toFixed(1)} GiB`
  if (mib >= 1) return `${mib.toFixed(1)} MiB`
  return `${Math.round(value)} KiB`
}

export function formatUptime(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds))
  const days = Math.floor(total / 86_400)
  const hours = Math.floor((total % 86_400) / 3_600)
  const minutes = Math.floor((total % 3_600) / 60)
  const secs = total % 60
  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0) parts.push(`${hours}h`)
  if (minutes > 0) parts.push(`${minutes}m`)
  if (secs > 0 || parts.length === 0) parts.push(`${secs}s`)
  return parts.join(' ')
}

export function formatPercent(value: number): string {
  return `${value.toFixed(1)}%`
}

export function formatLoad(value: number): string {
  return value.toFixed(2)
}

export function usagePercent(used: number, total: number): number {
  if (total <= 0) return 0
  return Math.min(100, Math.max(0, (used / total) * 100))
}

/** Load relative to the number of logical cores, clamped for a bar width. */
export function loadPercent(load: number, cores: number): number {
  if (cores <= 0) return 0
  return Math.min(100, Math.max(0, (load / cores) * 100))
}
