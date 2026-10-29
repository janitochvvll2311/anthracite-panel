import { LitElement, css, html, nothing } from 'lit'
import { customElement, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import { encodeBase64 } from 'anth-wire'
import { describeError } from 'anth-wire'
import { FILES_CHANGED_EVENT } from './file-editor'
import { formatBytes } from 'anth-wire'
import { parseErrorBody } from 'anth-wire'
import './icon'
import { baseName, join } from 'anth-wire'

type Direction = 'upload' | 'download'
type Status = 'queued' | 'running' | 'done' | 'failed' | 'cancelled'

interface TransferTask {
  id: number
  direction: Direction
  name: string
  path: string
  status: Status
  loaded: number
  total: number
  error: string
}

interface InternalTask extends TransferTask {
  directory?: string
  /** Encoded body kept for the replace retry, then released. */
  payload?: string
  xhr?: XMLHttpRequest
  start: () => void
}

const tasks: InternalTask[] = []
const listeners = new Set<() => void>()
let active: InternalTask | undefined
let nextId = 0
let panel: AnthTransfers | undefined

/** Registers a queued upload and shows it in the transfer panel. */
export function queueUpload(file: File, directory: string): void {
  const task: InternalTask = {
    id: nextId++,
    direction: 'upload',
    name: file.name,
    path: join(directory, file.name),
    directory,
    status: 'queued',
    loaded: 0,
    total: file.size,
    error: '',
    start: () => void performUpload(task, file, true),
  }
  enqueue(task)
}

/** Registers a queued download and shows it in the transfer panel. */
export function queueDownload(path: string): void {
  const task: InternalTask = {
    id: nextId++,
    direction: 'download',
    name: baseName(path),
    path,
    status: 'queued',
    loaded: 0,
    total: 0,
    error: '',
    start: () => performDownload(task),
  }
  enqueue(task)
}

/** Appends a task, shows the panel and starts it when the queue is idle. */
function enqueue(task: InternalTask): void {
  ensurePanel()
  tasks.push(task)
  notify()
  pump()
}

/** Snapshot for the panel and tests. */
export function transferSnapshot(): TransferTask[] {
  return tasks.map((task) => ({
    id: task.id,
    direction: task.direction,
    name: task.name,
    path: task.path,
    status: task.status,
    loaded: task.loaded,
    total: task.total,
    error: task.error,
  }))
}

/** Test helper: clears the singleton queue and its active slot. */
export function resetTransfers(): void {
  tasks.length = 0
  active = undefined
  notify()
}

function ensurePanel(): void {
  if (panel?.isConnected) {
    return
  }
  panel = document.createElement('anth-transfers') as AnthTransfers
  document.body.append(panel)
}

function notify(): void {
  for (const listener of listeners) {
    listener()
  }
}

/** The percentage the panel shows, or `null` when it cannot be known. */
function progressPercent(task: TransferTask): number | null {
  if (task.status === 'done') {
    return 100
  }
  if (task.total <= 0 || task.status === 'failed' || task.status === 'cancelled') {
    return null
  }
  return Math.min(100, Math.round((task.loaded / task.total) * 100))
}

/** Stores progress and repaints only when the visible percentage changes. */
function setProgress(task: InternalTask, loaded: number, total = task.total): void {
  const before = progressPercent(task)
  task.loaded = loaded
  task.total = total
  if (progressPercent(task) !== before) {
    notify()
  }
}

function pump(): void {
  if (active) {
    return
  }
  const next = tasks.find((task) => task.status === 'queued')
  if (!next) {
    return
  }
  active = next
  next.status = 'running'
  next.loaded = 0
  notify()
  next.start()
}

function finish(task: InternalTask, status: Status, error = ''): void {
  task.status = status
  task.error = error
  task.xhr = undefined
  task.payload = undefined
  if (status === 'done') {
    task.loaded = task.total
  }
  if (active === task) {
    active = undefined
  }
  notify()
  pump()
}

async function performUpload(task: InternalTask, file: File, create: boolean): Promise<void> {
  if (task.status !== 'running') {
    return
  }
  let payload = task.payload
  if (payload === undefined) {
    try {
      payload = encodeBase64(new Uint8Array(await file.arrayBuffer()))
      task.payload = payload
    } catch (error) {
      finish(task, 'failed', describeError(error))
      return
    }
  }
  if (task.status !== 'running') {
    return
  }

  const xhr = new XMLHttpRequest()
  task.xhr = xhr
  xhr.open('POST', '/api/files/upload')
  xhr.setRequestHeader('content-type', 'application/json')
  xhr.upload.onprogress = (event) => {
    if (!event.lengthComputable || event.total === 0) {
      return
    }
    setProgress(
      task,
      Math.min(task.total, Math.round((file.size * event.loaded) / event.total)),
    )
  }
  xhr.onload = () => {
    task.xhr = undefined
    if (xhr.status >= 200 && xhr.status < 300) {
      document.dispatchEvent(
        new CustomEvent(FILES_CHANGED_EVENT, {
          detail: { directory: task.directory, path: task.path },
        }),
      )
      finish(task, 'done')
      return
    }
    const failure = xhrFailure(xhr)
    if (create && xhr.status === 409 && failure.code === 'exists') {
      if (confirm(msg(str`${file.name} already exists. Replace it?`))) {
        task.loaded = 0
        void performUpload(task, file, false)
        return
      }
      finish(task, 'cancelled')
      return
    }
    finish(task, 'failed', failure.message)
  }
  xhr.onerror = () => finish(task, 'failed', msg('Network error'))
  xhr.onabort = () => finish(task, 'cancelled')
  xhr.send(JSON.stringify({ path: task.path, content_base64: payload, create }))
}

function performDownload(task: InternalTask): void {
  if (task.status !== 'running') {
    return
  }
  const xhr = new XMLHttpRequest()
  task.xhr = xhr
  xhr.open('GET', `/api/files/download?path=${encodeURIComponent(task.path)}`)
  xhr.responseType = 'blob'
  xhr.onprogress = (event) => {
    if (event.lengthComputable) {
      setProgress(task, event.loaded, event.total)
    }
  }
  xhr.onload = () => {
    task.xhr = undefined
    if (xhr.status >= 200 && xhr.status < 300) {
      const blob = xhr.response as Blob
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = task.name
      document.body.append(link)
      link.click()
      link.remove()
      setTimeout(() => URL.revokeObjectURL(url), 10_000)
      task.total = task.total || blob.size
      finish(task, 'done')
      return
    }
    void downloadFailure(xhr).then((message) => finish(task, 'failed', message))
  }
  xhr.onerror = () => finish(task, 'failed', msg('Network error'))
  xhr.onabort = () => finish(task, 'cancelled')
  xhr.send()
}

/** The API error a failed XHR carried, or its HTTP status. */
function xhrFailure(
  xhr: XMLHttpRequest,
  raw = xhr.responseText,
): { message: string; code?: string } {
  const body = parseErrorBody(raw)
  return {
    message: body.message ?? `${xhr.status} ${xhr.statusText}`.trim(),
    code: body.code,
  }
}

/** Downloads answer with a blob, so their error body needs a read first. */
async function downloadFailure(xhr: XMLHttpRequest): Promise<string> {
  try {
    return xhrFailure(xhr, await (xhr.response as Blob).text()).message
  } catch {
    return xhrFailure(xhr).message
  }
}

function cancelTask(id: number): void {
  const task = tasks.find((item) => item.id === id)
  if (!task) {
    return
  }
  if (task.status === 'queued') {
    task.status = 'cancelled'
    notify()
    pump()
    return
  }
  if (task.status === 'running') {
    task.xhr?.abort()
  }
}

function removeTask(id: number): void {
  const index = tasks.findIndex((item) => item.id === id)
  if (index >= 0 && tasks[index].status !== 'running') {
    tasks.splice(index, 1)
    notify()
  }
}

function clearFinished(): void {
  for (let index = tasks.length - 1; index >= 0; index -= 1) {
    if (tasks[index].status !== 'running' && tasks[index].status !== 'queued') {
      tasks.splice(index, 1)
    }
  }
  notify()
}

/**
 * The transfer panel. It lives on `document.body` so uploads and downloads
 * keep running while the Files panel is parked.
 */
@customElement('anth-transfers')
@localized()
export class AnthTransfers extends LitElement {
  @state()
  private snapshot: TransferTask[] = []

  @state()
  private collapsed = false

  private readonly onChange = (): void => {
    this.snapshot = transferSnapshot()
  }

  override connectedCallback(): void {
    super.connectedCallback()
    listeners.add(this.onChange)
    this.onChange()
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    listeners.delete(this.onChange)
  }

  private statusText(task: TransferTask): string {
    switch (task.status) {
      case 'queued':
        return msg('Queued')
      case 'running':
        return task.direction === 'upload' ? msg('Uploading') : msg('Downloading')
      case 'done':
        return msg('Done')
      case 'failed':
        return msg('Failed')
      case 'cancelled':
        return msg('Cancelled')
    }
  }

  override render() {
    if (this.snapshot.length === 0) {
      return nothing
    }
    const running = this.snapshot.filter(
      (task) => task.status === 'running' || task.status === 'queued',
    ).length
    return html`
      <section class="panel" aria-label=${msg('Transfers')}>
        <header>
          <span class="title">${msg('Transfers')}</span>
          ${running > 0 ? html`<span class="count">${running}</span>` : nothing}
          <span class="spacer"></span>
          <button
            type="button"
            class="icon-button"
            aria-label=${this.collapsed ? msg('Expand') : msg('Collapse')}
            title=${this.collapsed ? msg('Expand') : msg('Collapse')}
            @click=${() => (this.collapsed = !this.collapsed)}
          >
            <anth-icon name=${this.collapsed ? 'chevron-up' : 'chevron-down'}></anth-icon>
          </button>
          <button
            type="button"
            class="icon-button"
            aria-label=${msg('Clear finished')}
            title=${msg('Clear finished')}
            @click=${() => clearFinished()}
          >
            <anth-icon name="trash"></anth-icon>
          </button>
        </header>
        ${this.collapsed
          ? nothing
          : html`
              <ul>
                ${this.snapshot.map((task) => {
                  const percent = progressPercent(task)
                  return html`
                    <li data-status=${task.status}>
                      <div class="row">
                        <anth-icon
                          name=${task.direction === 'upload' ? 'upload' : 'download'}
                        ></anth-icon>
                        <span class="name" title=${task.path}>${task.name}</span>
                        <span class="status">${this.statusText(task)}</span>
                        ${task.status === 'running' || task.status === 'queued'
                          ? html`<button
                              type="button"
                              class="icon-button"
                              aria-label=${msg('Cancel')}
                              title=${msg('Cancel')}
                              @click=${() => cancelTask(task.id)}
                            >
                              <anth-icon name="close"></anth-icon>
                            </button>`
                          : html`<button
                              type="button"
                              class="icon-button"
                              aria-label=${msg('Clear')}
                              title=${msg('Clear')}
                              @click=${() => removeTask(task.id)}
                            >
                              <anth-icon name="close"></anth-icon>
                            </button>`}
                      </div>
                      <div
                        class="progress"
                        role="progressbar"
                        aria-valuemin="0"
                        aria-valuemax="100"
                        aria-valuenow=${percent ?? nothing}
                      >
                        <span style=${`width: ${percent ?? 0}%`}></span>
                      </div>
                      <div class="meta">
                        <span>${task.total > 0 ? formatBytes(task.total) : ''}</span>
                        ${percent === null
                          ? nothing
                          : html`<span>${percent}%</span>`}
                        ${task.error ? html`<span class="error">${task.error}</span>` : nothing}
                      </div>
                    </li>
                  `
                })}
              </ul>
            `}
      </section>
    `
  }

  static styles = css`
    :host {
      position: fixed;
      bottom: max(1rem, env(safe-area-inset-bottom));
      left: max(1rem, env(safe-area-inset-left));
      z-index: 60;
      display: block;
      width: min(24rem, calc(100vw - 2rem));
    }
    .panel {
      display: flex;
      flex-direction: column;
      max-height: min(50vh, 24rem);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      box-shadow: 0 18px 42px rgba(0, 0, 0, 0.45);
      overflow: hidden;
    }
    header {
      display: flex;
      flex: 0 0 auto;
      align-items: center;
      gap: 6px;
      padding: 8px 10px;
      border-bottom: 1px solid var(--panel-border, #223041);
    }
    .title {
      color: var(--text, #dbe6f2);
      font: 600 12px/1.3 var(--mono, monospace);
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }
    .count {
      min-width: 1.2rem;
      padding: 1px 5px;
      border-radius: 999px;
      background: rgba(125, 211, 252, 0.16);
      color: var(--accent, #7dd3fc);
      font: 600 10px/1.4 var(--mono, monospace);
      text-align: center;
    }
    .spacer { flex: 1 1 auto; }
    ul {
      display: grid;
      flex: 1 1 auto;
      gap: 2px;
      margin: 0;
      padding: 4px;
      overflow-y: auto;
      list-style: none;
    }
    li { display: grid; gap: 4px; padding: 6px 6px 7px; border-radius: 6px; }
    li[data-status='running'] { background: rgba(125, 211, 252, 0.06); }
    .row { display: flex; align-items: center; gap: 6px; min-width: 0; }
    .name {
      flex: 1 1 auto;
      min-width: 0;
      overflow: hidden;
      color: var(--text, #dbe6f2);
      font: 12px/1.4 var(--mono, monospace);
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .status {
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
      white-space: nowrap;
    }
    .icon-button {
      display: inline-grid;
      place-items: center;
      width: 26px;
      height: 26px;
      padding: 0;
      border: 1px solid transparent;
      border-radius: 5px;
      background: transparent;
      color: var(--muted, #8b9bad);
      font-size: 12px;
      cursor: pointer;
    }
    .icon-button:hover { border-color: var(--panel-border, #223041); color: var(--text, #dbe6f2); }
    .icon-button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 1px; }
    .progress {
      height: 4px;
      overflow: hidden;
      border-radius: 999px;
      background: rgba(139, 155, 173, 0.22);
    }
    .progress span {
      display: block;
      height: 100%;
      border-radius: inherit;
      background: linear-gradient(90deg, var(--accent, #7dd3fc), var(--ok, #34d399));
      transition: width 120ms linear;
    }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      color: var(--muted, #8b9bad);
      font: 10px/1.4 var(--mono, monospace);
    }
    .meta .error { color: var(--err, #f87171); }
    @media (max-width: 48rem) {
      /* Keep the FAB corner clear: a full-width panel at this height would
         swallow taps meant for the FAB and its menu. */
      :host {
        right: auto;
        bottom: calc(1rem + env(safe-area-inset-bottom));
        left: max(0.75rem, env(safe-area-inset-left));
        width: min(20rem, calc(100vw - 6.5rem));
      }
      .panel { max-height: min(45vh, 20rem); }
    }
    @media (prefers-reduced-motion: reduce) {
      .progress span { transition: none; }
    }
    @media (pointer: coarse) {
      .icon-button { width: 44px; height: 44px; }
    }
  `
}
