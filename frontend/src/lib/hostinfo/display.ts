import type { HostMemoryInfo } from './host-info-client'

/** Formats a duration in seconds the same way the CLI table does. */
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

export { formatKib } from '../format'

export function memoryUsedKib(memory: HostMemoryInfo): number {
  return Math.max(0, memory.total_kib - memory.available_kib)
}

export function swapUsedKib(memory: HostMemoryInfo): number {
  return Math.max(0, memory.swap_total_kib - memory.swap_free_kib)
}

/** Returns a clamped 0..100 percentage. */
export function usagePercent(used: number, total: number): number {
  if (total <= 0) return 0
  return Math.min(100, Math.max(0, (used / total) * 100))
}

/** Load relative to the number of logical cores, clamped for a bar width. */
export function loadPercent(load: number, cores: number): number {
  if (cores <= 0) return 0
  return Math.min(100, Math.max(0, (load / cores) * 100))
}

export function formatPercent(value: number): string {
  return `${value.toFixed(1)}%`
}

export function formatLoad(value: number): string {
  return value.toFixed(2)
}
