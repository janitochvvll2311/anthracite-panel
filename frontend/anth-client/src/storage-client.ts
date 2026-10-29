import type {
  Change,
  StorageClient,
  VolumeFormatRequest,
  VolumeMountRequest,
  VolumePartitionCreate,
  VolumePartitionDelete,
  VolumePartitionResize,
  VolumeSnapshot,
  VolumeUnmountRequest,
} from 'anth-wire'

import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpStorageClient implements StorageClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  get(): Promise<VolumeSnapshot> {
    return this.transport.json<VolumeSnapshot>('GET', '/api/volumes')
  }

  mount(request: VolumeMountRequest): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/volumes/mount', request)
  }

  unmount(request: VolumeUnmountRequest): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/volumes/unmount', request)
  }

  createPartition(request: VolumePartitionCreate): Promise<Change> {
    return this.transport.json<Change>(
      'POST',
      '/api/volumes/partition/create',
      request,
    )
  }

  resizePartition(request: VolumePartitionResize): Promise<Change> {
    return this.transport.json<Change>(
      'POST',
      '/api/volumes/partition/resize',
      request,
    )
  }

  deletePartition(request: VolumePartitionDelete): Promise<Change> {
    return this.transport.json<Change>(
      'POST',
      '/api/volumes/partition/delete',
      request,
    )
  }

  formatPartition(request: VolumeFormatRequest): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/volumes/format', request)
  }
}
