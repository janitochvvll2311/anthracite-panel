/**
 * HTTP client for the volume endpoints.
 *
 * The component layer never talks to fetch directly, so a different backend can
 * implement the same interface. Refresh is manual: there is deliberately no SSE
 * stream, because mounting and unmounting are operator actions, not telemetry.
 */

import { responseError } from '../http'
import type { HttpClientOptions } from '../http'

export type VolumeKind =
  | 'disk'
  | 'part'
  | 'crypt'
  | 'lvm'
  | 'raid'
  | 'loop'
  | 'rom'
  | 'other'

export interface VolumeDevice {
  name: string
  path: string
  kind: VolumeKind
  parent?: string
  size_bytes: number
  fstype?: string
  label?: string
  uuid?: string
  mountpoints: string[]
  read_only: boolean
  removable: boolean
  model?: string
  transport?: string
}

export interface VolumeUsage {
  total_bytes: number
  used_bytes: number
  available_bytes: number
}

export interface VolumeMountpoint {
  mountpoint: string
  source: string
  fstype: string
  options: string
  read_only: boolean
  virtual_fs: boolean
  usage?: VolumeUsage
}

export interface VolumeSnapshot {
  devices: VolumeDevice[]
  mounts: VolumeMountpoint[]
  control_enabled: boolean
  timestamp_ms: number
  total: number
  truncated: boolean
  warning?: string
}

export interface VolumeMountRequest {
  device: string
  target: string
  fs_type?: string
  read_only?: boolean
  allow_removable?: boolean
}

export interface VolumeClient {
  get: () => Promise<VolumeSnapshot>
  mount: (request: VolumeMountRequest) => Promise<string>
  unmount: (target: string) => Promise<string>
}

export class HttpVolumeClient implements VolumeClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch

  constructor(options: HttpClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api/volumes'
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
  }

  async get(): Promise<VolumeSnapshot> {
    const response = await this.fetchImpl(this.baseUrl, {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as VolumeSnapshot
  }

  async mount(request: VolumeMountRequest): Promise<string> {
    const response = await this.fetchImpl(`${this.baseUrl}/mount`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        device: request.device,
        target: request.target,
        fs_type: request.fs_type,
        read_only: request.read_only ?? false,
        allow_removable: request.allow_removable ?? false,
      }),
    })
    if (!response.ok) throw await responseError(response)
    const body = (await response.json()) as { message?: string }
    return body.message ?? 'mounted'
  }

  async unmount(target: string): Promise<string> {
    const response = await this.fetchImpl(`${this.baseUrl}/unmount`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ target }),
    })
    if (!response.ok) throw await responseError(response)
    const body = (await response.json()) as { message?: string }
    return body.message ?? 'unmounted'
  }
}
