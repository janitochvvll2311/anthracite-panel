import type { Change } from './actions'

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

export interface VolumeUnmountRequest {
  target: string
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
  mount(request: VolumeMountRequest): Promise<Change>
  unmount(request: VolumeUnmountRequest): Promise<Change>
  createPartition(request: VolumePartitionCreate): Promise<Change>
  resizePartition(request: VolumePartitionResize): Promise<Change>
  deletePartition(request: VolumePartitionDelete): Promise<Change>
  formatPartition(request: VolumeFormatRequest): Promise<Change>
}
