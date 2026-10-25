/**
 * How the panel presents what the wire carries.
 *
 * Reading the messages moved to `@anth/wire`, the counterpart of
 * `backend/anth-wire`, because that is the part a plugin or a harness also
 * needs and none of them should import the shell to get it. What stays here is
 * presentation: the units and the phrasing this panel shows. It mirrors no Rust
 * crate, and a second frontend could reasonably phrase these differently.
 */

/** Kibibytes as `KiB`/`MiB`/`GiB`, which is how the panel labels memory. */
export function formatKib(kib: number): string {
  // The argument is already in KiB, so the factors are in KiB too.
  const units: readonly [string, number][] = [
    ["GiB", 1024 * 1024],
    ["MiB", 1024],
  ];
  for (const [label, factor] of units) {
    if (kib >= factor) {
      const value = kib / factor;
      return `${value >= 10 ? value.toFixed(1) : value.toFixed(2)} ${label}`;
    }
  }
  return `${kib} KiB`;
}

/** A duration in seconds as `3d 4h`, `4h 12m`, `12m 30s` or `30s`. */
export function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const rest = Math.floor(seconds % 60);

  if (days > 0) {
    return `${days}d ${hours}h`;
  }
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m ${rest}s`;
  }
  return `${rest}s`;
}

/** Kibibytes of memory in use: total minus what the kernel counts as available. */
export function memoryUsedKib(memory: { total_kib: number; available_kib: number }): number {
  return Math.max(0, memory.total_kib - memory.available_kib);
}

/** Kibibytes of swap in use: total minus free. */
export function swapUsedKib(memory: { swap_total_kib: number; swap_free_kib: number }): number {
  return Math.max(0, memory.swap_total_kib - memory.swap_free_kib);
}

/**
 * `used` as a percentage of `total`, clamped to 0..100.
 *
 * A bar is painted from this, so a total of zero answers 0 rather than `NaN`:
 * `NaN%` is an invalid width and the bar would vanish instead of showing empty.
 */
export function usagePercent(used: number, total: number): number {
  if (total <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, (used / total) * 100));
}

/** Load relative to the cores available, clamped for a bar width. */
export function loadPercent(load: number, cores: number): number {
  if (cores <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, (load / cores) * 100));
}

/** A percentage as the panel prints it: one decimal, always `%`. */
export function formatPercent(value: number): string {
  return `${value.toFixed(1)}%`;
}

/** A load average as the panel prints it: two decimals, matching the CLI. */
export function formatLoad(value: number): string {
  return value.toFixed(2);
}
