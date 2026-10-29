import { responseError } from './http'

export type VolumeKind =
  | 'disk'
  | 'part'
  | 'crypt'
  | 'lvm'
  | 'raid'
  | 'loop'
  | 'rom'
  | 'other'

/** One block device as `/api/volumes` reports it. */
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

export interface VolumeMount {
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
  mounts: VolumeMount[]
  swap_devices: string[]
  control_enabled: boolean
  partition_enabled: boolean
  format_enabled: boolean
  mount_root: string
  disk_allow: string[]
  warning?: string
  timestamp_ms: number
}

export interface VolumeMountRequest {
  device: string
  target: string
  fs_type?: string
  read_only?: boolean
  allow_removable?: boolean
}

export type PartitionKind = 'linux' | 'swap' | 'efi'

export interface VolumePartitionCreate {
  disk: string
  kind: PartitionKind
  start_bytes?: number
  size_bytes: number
}

export interface VolumePartitionResize {
  disk: string
  number: number
  size_bytes: number
}

export interface VolumePartitionDelete {
  disk: string
  number: number
}

export interface VolumeFormatRequest {
  partition: string
  fs_type: string
  label?: string
}

export interface StorageClient {
  get(): Promise<VolumeSnapshot>
  mount(request: VolumeMountRequest): Promise<string>
  unmount(target: string): Promise<string>
  createPartition(request: VolumePartitionCreate): Promise<string>
  resizePartition(request: VolumePartitionResize): Promise<string>
  deletePartition(request: VolumePartitionDelete): Promise<string>
  formatPartition(request: VolumeFormatRequest): Promise<string>
}

export class HttpStorageClient implements StorageClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<VolumeSnapshot> {
    const response = await this.fetchImpl('/api/volumes')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as VolumeSnapshot
  }

  async mount(request: VolumeMountRequest): Promise<string> {
    return this.action('/api/volumes/mount', request)
  }

  async unmount(target: string): Promise<string> {
    return this.action('/api/volumes/unmount', { target })
  }

  async createPartition(request: VolumePartitionCreate): Promise<string> {
    return this.action('/api/volumes/partition/create', request)
  }

  async resizePartition(request: VolumePartitionResize): Promise<string> {
    return this.action('/api/volumes/partition/resize', request)
  }

  async deletePartition(request: VolumePartitionDelete): Promise<string> {
    return this.action('/api/volumes/partition/delete', request)
  }

  async formatPartition(request: VolumeFormatRequest): Promise<string> {
    return this.action('/api/volumes/format', request)
  }

  private async action(path: string, body: unknown): Promise<string> {
    const response = await this.fetchImpl(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await responseError(response)
    const result = (await response.json()) as { message?: string }
    return result.message ?? ''
  }
}
