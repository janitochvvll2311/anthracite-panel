/**
 * The messages that cross the browser boundary.
 *
 * This mirrors `anth_wire::message` in payload, not in envelope. The socket
 * carries a tagged `Request` because a connection has no verb and no URL; the
 * HTTP API names the command in the route and puts its arguments at the top
 * level, so there is no `type`/`data` envelope to reproduce here. Rebuilding one
 * would be the fabrication this contract exists to prevent: a `{"type":"host_info"}`
 * that no route accepts and no client sends.
 *
 * So the mirror is per message: one exported interface per thing that travels,
 * each named after the response variant it matches.
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

/** Operating system identification, from `os-release` and the kernel. */
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
