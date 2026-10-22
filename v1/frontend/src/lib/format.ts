const BYTE_UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB']

/** Formats a byte count with binary units. */
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

/** Formats a kibibyte count using KiB, MiB or GiB. */
export function formatKib(kib: number): string {
  const value = Math.max(0, Number.isFinite(kib) ? kib : 0)
  const mib = value / 1024
  if (mib >= 1024) return `${(mib / 1024).toFixed(1)} GiB`
  if (mib >= 1) return `${mib.toFixed(1)} MiB`
  return `${Math.round(value)} KiB`
}
