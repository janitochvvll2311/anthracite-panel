export function formatBytes(value: number): string {
  const size = Number(value) || 0;
  if (size < 1024) return `${size} B`;
  if (size < 1024 ** 2) return `${(size / 1024).toFixed(1)} KiB`;
  if (size < 1024 ** 3) return `${(size / 1024 ** 2).toFixed(1)} MiB`;
  if (size < 1024 ** 4) return `${(size / 1024 ** 3).toFixed(1)} GiB`;
  return `${(size / 1024 ** 4).toFixed(2)} TiB`;
}

export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  const days = Math.floor(total / 86400);
  const hours = Math.floor((total % 86400) / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  if (days) return `${days}d ${hours}h ${minutes}m`;
  if (hours) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

export function percent(used: number, total: number): number {
  const value = Number(total) > 0 ? (Number(used) / Number(total)) * 100 : 0;
  return Math.max(0, Math.min(100, value));
}
