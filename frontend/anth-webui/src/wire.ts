/**
 * The wire types, and the pure helpers that read them.
 *
 * Named after `anth-wire` on the Rust side, which is what these mirror: one
 * module holding the shapes that cross the boundary, never a transport. A client
 * decides how it fetches, so the same helpers serve the shell, a panel and a
 * test without any of them binding a port.
 *
 * They live here rather than in a package of their own because there is exactly
 * one consumer — this one. A `shared` package with a single dependant is a
 * folder, a manifest and an install step for nothing, and it has to be moved
 * again the moment the second consumer appears.
 */

/** `GET /api/host-info`, and `Response::HostInfo` on the socket. */
export interface HostInfo {
  hostname: string;
  os: OsInfo;
  uptime_seconds: number;
  cpu_model: string;
  logical_cores: number;
  load: LoadAverage;
  memory: MemoryInfo;
}

export interface OsInfo {
  name: string;
  version: string;
  kernel: string;
  arch: string;
}

export interface LoadAverage {
  one: number;
  five: number;
  fifteen: number;
}

export interface MemoryInfo {
  total_kib: number;
  available_kib: number;
  swap_total_kib: number;
  swap_free_kib: number;
}

/** Every failure the API returns is this shape, with the status in the line. */
export interface ApiErrorBody {
  error: string;
}

/**
 * The message from an error response, or its status line when the body is not
 * the documented shape. Never throws: reporting a failure must not fail.
 */
export async function responseError(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as Partial<ApiErrorBody>;
    if (typeof body.error === "string" && body.error.length > 0) {
      return body.error;
    }
  } catch {
    // A non-JSON body carries no detail worth a second error.
  }
  return `${response.status} ${response.statusText}`.trim();
}

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
