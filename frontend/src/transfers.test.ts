import { afterEach, beforeEach, expect, test, vi } from 'vitest'

import { FILES_CHANGED_EVENT } from './file-editor'
import {
  queueDownload,
  queueUpload,
  resetTransfers,
  transferSnapshot,
} from './transfers'

class FakeXhr {
  static instances: FakeXhr[] = []

  upload: { onprogress?: (event: ProgressEvent) => void } = {}

  onload?: () => void

  onerror?: () => void

  onabort?: () => void

  status = 0

  statusText = ''

  responseText = ''

  response: unknown = null

  open = vi.fn()

  setRequestHeader = vi.fn()

  send = vi.fn()

  abort = vi.fn(() => this.onabort?.())

  constructor() {
    FakeXhr.instances.push(this)
  }
}

const tick = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

beforeEach(() => {
  resetTransfers()
  FakeXhr.instances = []
  vi.stubGlobal('XMLHttpRequest', FakeXhr)
})

afterEach(() => {
  resetTransfers()
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

test('uploads queued files with progress and reports completion', async () => {
  const changed = vi.fn()
  document.addEventListener(FILES_CHANGED_EVENT, changed)

  queueUpload(new File(['hello bytes'], 'note.txt'), '/folder')
  await tick()

  const xhr = FakeXhr.instances[0]
  expect(xhr).toBeDefined()
  expect(xhr.open).toHaveBeenCalledWith('POST', '/api/files/upload')
  expect(transferSnapshot()[0]).toMatchObject({
    direction: 'upload',
    status: 'running',
    name: 'note.txt',
  })

  xhr.upload.onprogress?.({
    lengthComputable: true,
    loaded: 50,
    total: 100,
  } as ProgressEvent)
  expect(transferSnapshot()[0].loaded).toBe(6)

  xhr.status = 200
  xhr.onload?.()
  expect(transferSnapshot()[0].status).toBe('done')
  expect(changed).toHaveBeenCalledTimes(1)
  expect(changed.mock.calls[0][0].detail).toMatchObject({
    directory: '/folder',
    path: '/folder/note.txt',
  })
})

test('reports the API message when an upload fails', async () => {
  queueUpload(new File(['hello'], 'too-big.bin'), '/')
  await tick()

  const xhr = FakeXhr.instances[0]
  xhr.status = 413
  xhr.statusText = 'Payload Too Large'
  xhr.responseText = JSON.stringify({
    error: {
      code: 'too_large',
      message: 'the content is larger than the 4 byte limit',
    },
  })
  xhr.onload?.()

  expect(transferSnapshot()[0]).toMatchObject({
    status: 'failed',
    error: 'the content is larger than the 4 byte limit',
  })
})

test('reuses the encoded body when replacing an existing file', async () => {
  const file = new File(['hello'], 'note.txt')
  const read = vi.spyOn(file, 'arrayBuffer')
  vi.stubGlobal('confirm', vi.fn(() => true))

  queueUpload(file, '/')
  await tick()

  const first = FakeXhr.instances[0]
  first.status = 409
  first.responseText = JSON.stringify({
    error: { code: 'exists', message: 'note.txt already exists' },
  })
  first.onload?.()
  await tick()

  expect(FakeXhr.instances).toHaveLength(2)
  const second = FakeXhr.instances[1]
  expect(JSON.parse(second.send.mock.calls[0][0])).toEqual({
    path: '/note.txt',
    content_base64: 'aGVsbG8=',
    create: false,
  })
  expect(read).toHaveBeenCalledTimes(1)

  second.status = 200
  second.onload?.()
  expect(transferSnapshot()[0].status).toBe('done')
})

test('downloads queued files and marks them done', async () => {
  vi.stubGlobal('URL', {
    ...URL,
    createObjectURL: vi.fn(() => 'blob:test'),
    revokeObjectURL: vi.fn(),
  })
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})

  queueDownload('/folder/report.pdf')
  await tick()

  const xhr = FakeXhr.instances[0]
  expect(xhr).toBeDefined()
  expect(xhr.open).toHaveBeenCalledWith('GET', '/api/files/download?path=%2Ffolder%2Freport.pdf')

  const blob = new Blob(['download'], { type: 'text/plain' })
  xhr.response = blob
  xhr.status = 200
  xhr.onload?.()

  expect(transferSnapshot()[0]).toMatchObject({
    direction: 'download',
    status: 'done',
    total: blob.size,
    loaded: blob.size,
  })
})
