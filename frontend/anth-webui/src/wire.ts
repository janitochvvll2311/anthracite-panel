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
