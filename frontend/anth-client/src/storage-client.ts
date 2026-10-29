import { responseError } from './http'
import type { VolumeSnapshot, VolumeMountRequest, VolumePartitionCreate, VolumePartitionResize, VolumePartitionDelete, VolumeFormatRequest, StorageClient } from 'anth-wire'

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
