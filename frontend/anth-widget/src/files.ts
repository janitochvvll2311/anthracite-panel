import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { live } from 'lit/directives/live.js'
import { localized, msg, str } from '@lit/localize'

import { formatBytes } from './format'

import { describeError } from './errors'

import { openFileEditor } from './editor-window'
import { FILES_CHANGED_EVENT } from './file-editor'
import type {
  FileChunk,
  FileEntry,
  FilePage,
  FilesClient,
  TrashEntry,
  TrashListing,
} from 'anth-wire'
import type { ResponseError } from 'anth-client'
import { join, normalize, parent } from './paths'

import { queueDownload, queueUpload } from './transfers'
import './fab-menu'
import './icon'

const IMAGE_EXTENSIONS = new Set([
  'png',
  'jpg',
  'jpeg',
  'gif',
  'webp',
  'bmp',
  'avif',
  'ico',
  'svg',
])

function isImage(name: string): boolean {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  return IMAGE_EXTENSIONS.has(ext)
}

const ARCHIVE_SUFFIXES = ['.zip', '.tar', '.tar.gz', '.tgz', '.tar.xz', '.txz', '.7z']

function isArchive(name: string): boolean {
  const lower = name.toLowerCase()
  return ARCHIVE_SUFFIXES.some((suffix) => lower.endsWith(suffix))
}

function imageUrl(path: string): string {
  return `/api/files/image?path=${encodeURIComponent(path)}`
}

function iconFor(kind: string): string {
  if (kind === 'dir') return 'files'
  if (kind === 'symlink') return 'link'
  return 'file'
}

function sizeLabel(entry: FileEntry): string {
  return entry.kind === 'file' ? formatBytes(entry.size) : '-'
}

function modifiedLabel(entry: FileEntry): string {
  return entry.modified_ms > 0 ? new Date(entry.modified_ms).toLocaleString() : '-'
}

@customElement('anth-files')
@localized()
export class AnthFiles extends LitElement {
  @property({ attribute: false })
  client?: FilesClient

  @state()
  private path = '/'

  @state()
  private entries: FileEntry[] = []

  @state()
  private next: string | null = null

  @state()
  private previewEntry?: FileEntry

  @state()
  private chunk?: FileChunk

  @state()
  private previewPath = ''

  @state()
  private previewUnavailable = false

  @state()
  private imageFailed = false

  @state()
  private confirmingDelete = false

  @state()
  private error = ''

  @state()
  private notice = ''

  @state()
  private busy = false

  @state()
  private editingPath = false

  @state()
  private pathDraft = ''

  @state()
  private trashEnabled = false

  @state()
  private trash?: TrashListing

  @state()
  private trashView = false

  override connectedCallback(): void {
    super.connectedCallback()
    document.addEventListener(FILES_CHANGED_EVENT, this.onFilesChanged)
    queueMicrotask(() => {
      void this.open(this.path)
      void this.refreshTrash()
    })
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    document.removeEventListener(FILES_CHANGED_EVENT, this.onFilesChanged)
  }

  /** Lists one directory and clears any open preview. */
  private async open(path: string): Promise<void> {
    if (!this.client) {
      this.error = msg('No files client was configured')
      return
    }
    this.trashView = false
    this.busy = true
    this.notice = ''
    try {
      const target = normalize(path)
      const page: FilePage = await this.client.list(target)
      this.path = target
      this.entries = page.items
      this.next = page.next ?? null
      this.resetPreview()
      this.error = ''
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private async more(): Promise<void> {
    if (!this.client || !this.next) {
      return
    }
    this.busy = true
    try {
      const page = await this.client.list(this.path, this.next)
      this.entries = [...this.entries, ...page.items]
      this.next = page.next ?? null
      this.error = ''
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private async preview(entry: FileEntry): Promise<void> {
    if (!this.client) {
      return
    }
    const path = join(this.path, entry.name)
    this.resetPreview()
    this.previewEntry = entry
    this.previewPath = path
    this.error = ''
    this.busy = true
    try {
      if (!isImage(entry.name)) {
        this.chunk = await this.client.read(path)
      }
    } catch {
      // Binary files, permissions and transport errors all land on the same
      // generic placeholder; the details and the actions still work.
      this.previewUnavailable = true
    } finally {
      this.busy = false
    }
  }

  /** Clears the preview and every flag tied to it. */
  private resetPreview(): void {
    this.previewEntry = undefined
    this.chunk = undefined
    this.previewPath = ''
    this.previewUnavailable = false
    this.imageFailed = false
    this.confirmingDelete = false
  }

  private closePreview(): void {
    this.resetPreview()
  }

  /** Reads the trash so the toolbar and the trash view stay current. */
  private async refreshTrash(): Promise<void> {
    const client = this.client
    if (!client) {
      return
    }
    try {
      const listing = await client.trashList()
      this.trash = listing
      this.trashEnabled = listing.enabled
    } catch {
      // An older daemon without the trash route keeps the permanent delete.
      this.trashEnabled = false
    }
  }

  private showTrash(): void {
    this.resetPreview()
    this.trashView = true
    this.notice = ''
    void this.refreshTrash()
  }

  private async showFiles(): Promise<void> {
    this.trashView = false
    await this.open(this.path)
  }

  private async restoreEntry(entry: TrashEntry): Promise<void> {
    const client = this.client
    if (!client) {
      return
    }
    this.busy = true
    try {
      try {
        await client.restore(entry.id, false)
      } catch (error) {
        if ((error as ResponseError).code !== 'exists') {
          throw error
        }
        if (!confirm(msg(str`${entry.original_path} already exists. Replace it?`))) {
          return
        }
        await client.restore(entry.id, true)
      }
      this.notice = msg(str`Restored ${entry.original_path}`)
      await this.refreshTrash()
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private async emptyTrash(): Promise<void> {
    const client = this.client
    if (!client) {
      return
    }
    if (!confirm(msg('Delete everything in the trash?'))) {
      return
    }
    this.busy = true
    try {
      await client.emptyTrash()
      this.notice = msg('Trash emptied')
      await this.refreshTrash()
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private async deleteFile(): Promise<void> {
    const client = this.client
    if (!client || !this.previewPath) {
      return
    }
    const path = this.previewPath
    this.busy = true
    try {
      const trashed = this.trashEnabled
      if (trashed) {
        await client.trash(path)
      } else {
        await client.delete(path)
      }
      const directory = this.path
      this.closePreview()
      await this.open(directory)
      this.notice = trashed ? msg(str`Moved ${path} to the trash`) : msg(str`Deleted ${path}`)
      if (trashed) {
        await this.refreshTrash()
      }
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  /** Compresses the selected entry beside itself. */
  private async compress(entry: FileEntry): Promise<void> {
    await this.compressPath(join(this.path, entry.name))
  }

  private async compressCurrent(): Promise<void> {
    await this.compressPath(this.path)
  }

  private async compressPath(path: string): Promise<void> {
    const client = this.client
    if (!client) {
      return
    }
    this.busy = true
    try {
      const created = await client.archive(path)
      await this.open(this.path)
      this.notice = msg(str`Created ${created}`)
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  /** Extracts an archive beside itself. */
  private async extract(entry: FileEntry): Promise<void> {
    const client = this.client
    if (!client) {
      return
    }
    this.busy = true
    try {
      const result = await client.extractArchive(join(this.path, entry.name))
      await this.open(this.path)
      this.notice = msg(str`Extracted into ${result.path}`)
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private async morePreview(): Promise<void> {
    if (!this.client || !this.chunk) {
      return
    }
    this.busy = true
    try {
      const next = await this.client.read(
        this.previewPath,
        this.chunk.offset + this.chunk.text.length,
      )
      this.chunk = { ...next, text: this.chunk.text + next.text }
      this.error = ''
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private activate(entry: FileEntry): void {
    if (entry.kind === 'dir') {
      void this.open(join(this.path, entry.name))
    } else {
      void this.preview(entry)
    }
  }

  private edit(entry: FileEntry): void {
    if (!this.client) {
      return
    }
    openFileEditor({
      client: this.client,
      mode: 'edit',
      directory: this.path,
      name: entry.name,
    })
  }

  private create(): void {
    if (!this.client) {
      return
    }
    openFileEditor({ client: this.client, mode: 'create', directory: this.path, name: '' })
  }

  private closeFileMenu(): void {
    this.shadowRoot?.querySelector('anth-fab-menu')?.removeAttribute('open')
  }

  /** Queues one task per selected file; the picker stays usable. */
  private onUpload(event: Event): void {
    const input = event.target as HTMLInputElement
    const files = [...(input.files ?? [])]
    input.value = ''
    this.closeFileMenu()
    if (files.length === 0) {
      return
    }
    this.error = ''
    for (const file of files) {
      queueUpload(file, this.path)
    }
  }

  /** A global editor wrote a file; reload the listing and its preview. */
  private readonly onFilesChanged = (event: Event): void => {
    const detail = (event as CustomEvent<{ directory?: string; path?: string }>).detail
    if (detail?.directory && normalize(detail.directory) !== this.path) {
      return
    }
    void this.refreshAfterSave(detail?.path)
  }

  private async refreshAfterSave(path?: string): Promise<void> {
    await this.open(this.path)
    if (!path) {
      return
    }
    const entry = this.entries.find((item) => join(this.path, item.name) === path)
    if (entry) {
      await this.preview(entry)
    }
  }

  private up(): void {
    if (this.path !== '/') {
      void this.open(parent(this.path))
    }
  }

  private crumbs(): { label: string; path: string }[] {
    const parts = this.path.split('/').filter(Boolean)
    let accumulated = ''
    return [
      { label: '/', path: '/' },
      ...parts.map((part) => {
        accumulated = `${accumulated}/${part}`
        return { label: part, path: accumulated }
      }),
    ]
  }

  private kindLabel(kind: string): string {
    switch (kind) {
      case 'dir':
        return msg('Folder')
      case 'file':
        return msg('File')
      case 'symlink':
        return msg('Link')
      default:
        return msg('Other')
    }
  }

  private startPathEdit(): void {
    this.pathDraft = this.path
    this.editingPath = true
    void this.updateComplete.then(() => {
      const input = this.shadowRoot?.querySelector<HTMLInputElement>('.path-edit input')
      input?.focus()
      input?.select()
    })
  }

  private cancelPathEdit(): void {
    this.editingPath = false
  }

  private async submitPath(event: Event): Promise<void> {
    event.preventDefault()
    const target = normalize(this.pathDraft)
    this.editingPath = false
    await this.open(target)
  }

  private onPathInput(event: Event): void {
    this.pathDraft = (event.target as HTMLInputElement).value
  }

  private onPathKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.stopPropagation()
      this.cancelPathEdit()
    }
  }

  private renderPath() {
    if (this.editingPath) {
      return html`
        <form class="path-edit" @submit=${this.submitPath}>
          <input
            .value=${live(this.pathDraft)}
            aria-label=${msg('Path')}
            autocomplete="off"
            spellcheck="false"
            @input=${this.onPathInput}
            @keydown=${this.onPathKeydown}
            @blur=${() => this.cancelPathEdit()}
          />
        </form>
      `
    }
    const crumbs = this.crumbs()
    return html`
      <nav class="crumbs" aria-label=${msg('Path')}>
        ${crumbs.map((crumb, index) => {
          const current = crumb.path === this.path
          return html`
            ${index > 1 ? html`<span class="sep" aria-hidden="true">/</span>` : nothing}
            <button
              type="button"
              class="crumb"
              aria-current=${current ? 'page' : nothing}
              title=${current ? msg('Edit path') : nothing}
              ?disabled=${this.busy}
              @click=${() => (current ? this.startPathEdit() : this.open(crumb.path))}
            >
              ${crumb.label}
            </button>
          `
        })}
      </nav>
    `
  }

  private renderListing() {
    if (this.entries.length === 0) {
      return html`<p class="empty">${msg('This folder is empty.')}</p>`
    }
    return html`
      <div class="rows">
        <div class="row head" aria-hidden="true">
          <span>${msg('Name')}</span>
          <span>${msg('Kind')}</span>
          <span class="num">${msg('Size')}</span>
          <span class="num">${msg('Modified')}</span>
        </div>
        ${this.entries.map(
          (entry) => html`
            <div class="row">
              <button type="button" class="entry" @click=${() => this.activate(entry)}>
                <anth-icon name=${iconFor(entry.kind)}></anth-icon>
                <span>${entry.name}${entry.kind === 'dir' ? '/' : ''}</span>
              </button>
              <span>${this.kindLabel(entry.kind)}</span>
              <span class="num">${sizeLabel(entry)}</span>
              <span class="num">${modifiedLabel(entry)}</span>
            </div>
          `,
        )}
      </div>
    `
  }

  private renderPreview() {
    const entry = this.previewEntry
    if (!entry) {
      return nothing
    }
    const image = entry.kind === 'file' && isImage(entry.name)
    return html`
      <aside class="preview">
        <header>
          <span class="title" title=${this.previewPath}>${entry.name}</span>
          <div class="actions">
            ${entry.kind === 'file'
              ? html`<button
                  type="button"
                  aria-label=${msg('Download')}
                  @click=${() => queueDownload(this.previewPath)}
                >
                  <anth-icon name="download"></anth-icon>
                  <span class="btn-label">${msg('Download')}</span>
                </button>`
              : nothing}
            ${entry.kind === 'file' && !image
              ? html`<button
                  type="button"
                  aria-label=${msg('Edit')}
                  ?disabled=${this.busy}
                  @click=${() => void this.edit(entry)}
                >
                  <anth-icon name="edit"></anth-icon>
                  <span class="btn-label">${msg('Edit')}</span>
                </button>`
              : nothing}
            ${entry.kind === 'file' || entry.kind === 'dir'
              ? html`<button
                  type="button"
                  aria-label=${msg('Compress')}
                  ?disabled=${this.busy}
                  @click=${() => void this.compress(entry)}
                >
                  <anth-icon name="archive"></anth-icon>
                  <span class="btn-label">${msg('Compress')}</span>
                </button>`
              : nothing}
            ${entry.kind === 'file' && isArchive(entry.name)
              ? html`<button
                  type="button"
                  aria-label=${msg('Extract')}
                  ?disabled=${this.busy}
                  @click=${() => void this.extract(entry)}
                >
                  <anth-icon name="archive"></anth-icon>
                  <span class="btn-label">${msg('Extract')}</span>
                </button>`
              : nothing}
            ${entry.kind === 'file' || entry.kind === 'symlink'
              ? html`<button
                  type="button"
                  class="danger"
                  aria-label=${msg('Delete')}
                  ?disabled=${this.busy}
                  @click=${() => (this.confirmingDelete = true)}
                >
                  <anth-icon name="trash"></anth-icon>
                  <span class="btn-label">${msg('Delete')}</span>
                </button>`
              : nothing}
            <button type="button" aria-label=${msg('Close')} @click=${() => this.closePreview()}>
              <anth-icon name="close"></anth-icon>
              <span class="btn-label">${msg('Close')}</span>
            </button>
          </div>
        </header>
        ${this.confirmingDelete
          ? html`
              <div class="confirm" role="alert">
                <span>${this.trashEnabled ? msg('Move this file to the trash?') : msg('Delete this file?')}</span>
                <button
                  type="button"
                  class="danger"
                  aria-label=${this.trashEnabled ? msg('Move to trash') : msg('Delete')}
                  ?disabled=${this.busy}
                  @click=${() => void this.deleteFile()}
                >
                  <anth-icon name="trash"></anth-icon>
                  <span class="btn-label">${this.trashEnabled ? msg('Move to trash') : msg('Delete')}</span>
                </button>
                <button
                  type="button"
                  aria-label=${msg('Cancel')}
                  ?disabled=${this.busy}
                  @click=${() => (this.confirmingDelete = false)}
                >
                  <anth-icon name="close"></anth-icon>
                  <span class="btn-label">${msg('Cancel')}</span>
                </button>
              </div>
            `
          : nothing}
        <dl class="details">
          <div>
            <dt>${msg('Path')}</dt>
            <dd><code>${this.previewPath}</code></dd>
          </div>
          <div>
            <dt>${msg('Kind')}</dt>
            <dd>${this.kindLabel(entry.kind)}</dd>
          </div>
          <div>
            <dt>${msg('Size')}</dt>
            <dd>${sizeLabel(entry)}</dd>
          </div>
          <div>
            <dt>${msg('Modified')}</dt>
            <dd>${modifiedLabel(entry)}</dd>
          </div>
        </dl>
        <div class="content">
          ${image
            ? this.imageFailed
              ? html`
                  <div class="placeholder" role="status">
                    <span class="glyph" aria-hidden="true">▤</span>
                    <p>${msg('Preview not available for this file.')}</p>
                  </div>
                `
              : html`
                  <div class="image">
                    <img
                      src=${imageUrl(this.previewPath)}
                      alt=${entry.name}
                      @error=${() => (this.imageFailed = true)}
                    />
                  </div>
                `
            : this.chunk
              ? html`
                  <pre>${this.chunk.text}</pre>
                  ${this.chunk.eof
                    ? nothing
                    : html`<button
                        type="button"
                        class="more"
                        aria-label=${msg('Load more')}
                        ?disabled=${this.busy}
                        @click=${() => this.morePreview()}
                      >
                        <anth-icon name="chevron-down"></anth-icon>
                        <span class="btn-label">${msg('Load more')}</span>
                      </button>`}
                `
              : this.previewUnavailable
                ? html`
                    <div class="placeholder" role="status">
                      <span class="glyph" aria-hidden="true">▤</span>
                      <p>${msg('Preview not available for this file.')}</p>
                    </div>
                  `
                : html`<p class="placeholder">${msg('Loading…')}</p>`}
        </div>
      </aside>
    `
  }

  private renderTrash() {
    const listing = this.trash
    if (!listing) {
      return html`<p class="empty">${msg('Loading…')}</p>`
    }
    const limit = listing.max_bytes ? ` / ${formatBytes(listing.max_bytes)}` : ''
    return html`
      <section class="trash" aria-label=${msg('Trash')}>
        <header>
          <div>
            <h2>${msg('Trash')}</h2>
            <span class="trash-meta">
              ${formatBytes(listing.used_bytes)}${limit} · ${listing.total}
            </span>
          </div>
          <button
            type="button"
            class="danger"
            aria-label=${msg('Empty trash')}
            ?disabled=${this.busy || listing.entries.length === 0}
            @click=${() => void this.emptyTrash()}
          >
            <anth-icon name="trash"></anth-icon>
            <span class="btn-label">${msg('Empty trash')}</span>
          </button>
        </header>
        ${listing.entries.length === 0
          ? html`<p class="empty">${msg('The trash is empty.')}</p>`
          : html`
              <ul>
                ${listing.entries.map(
                  (entry) => html`
                    <li>
                      <anth-icon name=${iconFor(entry.kind)}></anth-icon>
                      <div class="trash-meta">
                        <span class="name" title=${entry.original_path}>${entry.name}</span>
                        <code>${entry.original_path}</code>
                        <span class="sub">
                          ${formatBytes(entry.size)}${entry.deleted_at
                            ? ` · ${entry.deleted_at}`
                            : ''}
                        </span>
                      </div>
                      <button
                        type="button"
                        aria-label=${msg('Restore')}
                        ?disabled=${this.busy}
                        @click=${() => void this.restoreEntry(entry)}
                      >
                        <anth-icon name="undo"></anth-icon>
                        <span class="btn-label">${msg('Restore')}</span>
                      </button>
                    </li>
                  `,
                )}
              </ul>
            `}
        ${listing.warning ? html`<p class="warning">${listing.warning}</p>` : nothing}
      </section>
    `
  }

  override render() {
    return html`
      <section class="files">
        <header class="bar">
          ${this.renderPath()}
          <div class="tools">
            ${this.trashEnabled
              ? html`<button
                  type="button"
                  aria-label=${this.trashView ? msg('Files') : msg('Trash')}
                  ?disabled=${this.busy}
                  @click=${() => (this.trashView ? void this.showFiles() : this.showTrash())}
                >
                  <anth-icon name=${this.trashView ? 'files' : 'trash'}></anth-icon>
                  <span class="btn-label">${this.trashView ? msg('Files') : msg('Trash')}</span>
                  ${!this.trashView && this.trash && this.trash.total > 0
                    ? html`<span class="badge">${this.trash.total}</span>`
                    : nothing}
                </button>`
              : nothing}
            <button
              type="button"
              aria-label=${msg('Up')}
              ?disabled=${this.busy || this.trashView || this.path === '/'}
              @click=${() => this.up()}
            >
              <anth-icon name="up"></anth-icon>
              <span class="btn-label">${msg('Up')}</span>
            </button>
            <button
              type="button"
              aria-label=${msg('Reload')}
              ?disabled=${this.busy}
              @click=${() => (this.trashView ? void this.refreshTrash() : this.open(this.path))}
            >
              <anth-icon name="refresh"></anth-icon>
              <span class="btn-label">${msg('Reload')}</span>
            </button>
          </div>
        </header>

        ${this.error ? html`<div class="error" role="alert">${this.error}</div>` : nothing}
        ${this.notice ? html`<div class="notice" role="status">${this.notice}</div>` : nothing}

        ${this.trashView
          ? this.renderTrash()
          : html`
              <div class="layout ${this.previewEntry ? 'preview-open' : ''}">
                <div class="listing">
                  ${this.busy && this.entries.length === 0
                    ? html`<p class="empty">${msg('Loading…')}</p>`
                    : this.renderListing()}
                  ${this.next
                    ? html`<button
                        type="button"
                        class="more"
                        aria-label=${msg('Load more')}
                        ?disabled=${this.busy}
                        @click=${() => this.more()}
                      >
                        <anth-icon name="chevron-down"></anth-icon>
                        <span class="btn-label">${msg('Load more')}</span>
                      </button>`
                    : nothing}
                </div>
                ${this.renderPreview()}
              </div>
            `}
        ${this.trashView
          ? nothing
          : html`
              <anth-fab-menu icon="plus" label=${msg('File actions')}>
                <button type="button" class="fab-action" aria-label=${msg('New file')} @click=${() => this.create()}>
                  <anth-icon name="file-plus"></anth-icon>
                  <span class="btn-label">${msg('New file')}</span>
                </button>
                <button
                  type="button"
                  class="fab-action"
                  aria-label=${msg('Compress folder')}
                  ?disabled=${this.path === '/'}
                  @click=${() => void this.compressCurrent()}
                >
                  <anth-icon name="archive"></anth-icon>
                  <span class="btn-label">${msg('Compress folder')}</span>
                </button>
                <label class="fab-action upload-action">
                  <input
                    class="picker"
                    type="file"
                    multiple
                    aria-label=${msg('Upload from device')}
                    @change=${this.onUpload}
                    @cancel=${() => this.closeFileMenu()}
                  />
                  <anth-icon name="upload"></anth-icon>
                  <span class="btn-label">${msg('Upload from device')}</span>
                </label>
              </anth-fab-menu>
            `}
      </section>
    `
  }

  static styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 14px/1.5 var(--sans, system-ui, sans-serif);
    }
    .files {
      --pane-height: max(14rem, calc(100dvh - 16rem));
      position: relative;
      display: grid;
      gap: 12px;
      min-width: 0;
    }
    .bar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }
    .crumbs { display: flex; flex-wrap: wrap; align-items: center; gap: 2px; min-width: 0; }
    .crumb, .tools button, .more, .preview button, .confirm button, .trash button {
      border: 1px solid transparent;
      border-radius: 6px;
      background: transparent;
      color: var(--text, #dbe6f2);
      padding: 4px 7px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .crumb:hover, .tools button:hover, .more:hover, .preview button:hover, .confirm button:hover, .trash button:hover {
      border-color: var(--panel-border, #223041);
      background: rgba(125, 211, 252, 0.06);
    }
    .crumb[aria-current='page'] { color: var(--accent, #7dd3fc); }
    .sep { color: var(--muted, #8b9bad); }
    .path-edit { flex: 1 1 auto; min-width: 0; }
    .path-edit input {
      box-sizing: border-box;
      width: 100%;
      border: 1px solid var(--accent, #7dd3fc);
      border-radius: 6px;
      background: var(--panel-2, #0b1119);
      color: var(--text, #dbe6f2);
      padding: 4px 7px;
      font: 12px/1.4 var(--mono, monospace);
    }
    .path-edit input:focus { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 1px; }
    .tools { display: flex; gap: 6px; }
    .tools button, .more, .preview button, .confirm button, .trash button, .fab-action {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      border-color: var(--panel-border, #223041);
      background: rgba(125, 211, 252, 0.08);
    }
    .fab-action { min-height: 38px; }
    .upload-action {
      position: relative;
      overflow: hidden;
    }
    .picker {
      position: absolute;
      inset: 0;
      width: 100%;
      height: 100%;
      margin: 0;
      padding: 0;
      border: 0;
      opacity: 0;
      cursor: pointer;
      font-size: 0;
    }
    anth-icon { font-size: 14px; }
    button:disabled { opacity: 0.5; cursor: default; }
    button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }

    .layout { display: grid; gap: 12px; min-width: 0; }
    .listing {
      min-width: 0;
      max-height: var(--pane-height);
      overflow: auto;
      overscroll-behavior: contain;
    }
    .rows { min-width: 0; }
    .row {
      display: grid;
      grid-template-columns: minmax(0, 1fr) 5.5rem 5.5rem 10rem;
      gap: 0 0.75rem;
      align-items: baseline;
      padding: 6px 8px;
      border-bottom: 1px solid var(--panel-border, #223041);
      font: 13px/1.45 var(--mono, monospace);
    }
    .row.head {
      color: var(--muted, #8b9bad);
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
    .row > :first-child { min-width: 0; }
    .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
    .entry {
      display: flex;
      width: 100%;
      align-items: center;
      gap: 6px;
      border: 0;
      padding: 0;
      background: transparent;
      color: var(--accent, #7dd3fc);
      font: inherit;
      text-align: left;
      overflow-wrap: anywhere;
      cursor: pointer;
    }
    .entry span { min-width: 0; overflow-wrap: anywhere; }
    .entry anth-icon { font-size: 14px; color: var(--muted, #8b9bad); }
    .entry:hover { text-decoration: underline; }
    .empty {
      margin: 0;
      padding: 1.5rem 0.75rem;
      text-align: center;
      color: var(--muted, #8b9bad);
      border: 1px dashed var(--panel-border, #223041);
      border-radius: 8px;
    }
    .error {
      margin: 0;
      padding: 0.6rem 0.75rem;
      border-radius: 8px;
      border: 1px solid rgba(248, 113, 113, 0.4);
      background: rgba(248, 113, 113, 0.12);
      color: var(--err, #f87171);
    }
    .notice {
      margin: 0;
      padding: 0.6rem 0.75rem;
      border-radius: 8px;
      border: 1px solid rgba(52, 211, 153, 0.4);
      background: rgba(52, 211, 153, 0.12);
      color: #a7f3d0;
    }
    .badge {
      min-width: 1.1rem;
      margin-left: 2px;
      padding: 0 4px;
      border-radius: 999px;
      background: rgba(125, 211, 252, 0.18);
      color: var(--accent, #7dd3fc);
      font: 600 10px/1.5 var(--mono, monospace);
      text-align: center;
    }
    .trash {
      display: flex;
      flex-direction: column;
      gap: 10px;
      max-height: var(--pane-height);
      padding: 10px;
      overflow-y: auto;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
    }
    .trash > header {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }
    .trash h2 { margin: 0; font-size: 14px; }
    .trash-meta {
      display: flex;
      flex-direction: column;
      gap: 2px;
      min-width: 0;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
    }
    .trash-meta .name {
      overflow: hidden;
      color: var(--text, #dbe6f2);
      font: 12px/1.4 var(--mono, monospace);
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .trash-meta code {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .trash ul {
      display: grid;
      gap: 2px;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .trash li {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 8px 4px;
      border-top: 1px solid var(--panel-border, #223041);
    }
    .trash li .trash-meta { flex: 1 1 auto; }
    .trash .warning { margin: 0; color: #fbbf24; font-size: 12px; }
    .preview {
      display: flex;
      flex-direction: column;
      min-width: 0;
      max-height: var(--pane-height);
      box-sizing: border-box;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
      padding: 10px;
    }
    .preview header { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
    .preview .title {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font: 12px/1.4 var(--mono, monospace);
      color: var(--text, #dbe6f2);
    }
    .preview .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 6px; }
    .preview .danger, .trash .danger { border-color: rgba(248, 113, 113, 0.4); color: var(--err, #f87171); }
    .preview .danger:hover, .trash .danger:hover { background: rgba(248, 113, 113, 0.12); }
    .confirm {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
      margin-top: 8px;
      padding: 6px 8px;
      border: 1px solid rgba(248, 113, 113, 0.4);
      border-radius: 6px;
      background: rgba(248, 113, 113, 0.1);
      font: 12px/1.4 var(--mono, monospace);
    }
    .confirm span { margin-right: auto; }
    .details {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr);
      gap: 2px 10px;
      margin: 8px 0 0;
      font: 12px/1.5 var(--mono, monospace);
    }
    .details > div { display: contents; }
    .details dt { color: var(--muted, #8b9bad); }
    .details dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
    .preview .content {
      flex: 1 1 auto;
      min-height: 0;
      overflow: auto;
      overscroll-behavior: contain;
      margin-top: 8px;
    }
    .preview .image {
      display: flex;
      align-items: flex-start;
      justify-content: center;
      min-height: 0;
    }
    .preview .image img {
      max-width: 100%;
      height: auto;
      border-radius: 4px;
      background:
        linear-gradient(45deg, rgba(139, 155, 173, 0.08) 25%, transparent 25%),
        linear-gradient(-45deg, rgba(139, 155, 173, 0.08) 25%, transparent 25%),
        linear-gradient(45deg, transparent 75%, rgba(139, 155, 173, 0.08) 75%),
        linear-gradient(-45deg, transparent 75%, rgba(139, 155, 173, 0.08) 75%);
      background-size: 16px 16px;
      background-position: 0 0, 0 8px, 8px -8px, -8px 0;
    }
    .preview pre {
      margin: 0;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
      font: 12px/1.5 var(--mono, monospace);
      color: var(--text, #dbe6f2);
    }
    .preview code { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
    .placeholder {
      display: grid;
      place-items: center;
      gap: 6px;
      margin: 0;
      padding: 2rem 0.75rem;
      text-align: center;
      color: var(--muted, #8b9bad);
      border: 1px dashed var(--panel-border, #223041);
      border-radius: 8px;
    }
    .placeholder p { margin: 0; }
    .placeholder .glyph { font-size: 1.5rem; line-height: 1; }
    .more { justify-self: start; margin-top: 8px; }

    @media (min-width: 48rem) {
      .layout.preview-open { grid-template-columns: minmax(0, 1fr) minmax(18rem, 32rem); }
      /* With the preview open the listing loses width; keep names readable and
         move the metadata to a second line instead of squeezing the table. */
      .layout.preview-open .row.head { display: none; }
      .layout.preview-open .row {
        grid-template-columns: minmax(0, 1fr) auto;
        grid-template-areas:
          "name name"
          "kind modified";
        row-gap: 0.15rem;
        padding: 0.5rem 0.35rem;
      }
      .layout.preview-open .row > :nth-child(1) { grid-area: name; }
      .layout.preview-open .row > :nth-child(2) {
        grid-area: kind;
        color: var(--muted, #8b9bad);
        font-size: 11px;
      }
      .layout.preview-open .row > :nth-child(3) { display: none; }
      .layout.preview-open .row > :nth-child(4) {
        grid-area: modified;
        color: var(--muted, #8b9bad);
        font-size: 11px;
      }
    }
    @media (max-width: 48rem) {
      /* Below this width the listing stacks so a name keeps the full row, and
         the preview and the editor take the panel over instead of sitting
         beside it. */
      .row.head { display: none; }
      .row {
        grid-template-columns: minmax(0, 1fr) auto;
        grid-template-areas:
          "name name"
          "kind modified";
        gap: 0.15rem 0.5rem;
        padding: 0.55rem 0.35rem;
      }
      .row > :nth-child(1) { grid-area: name; }
      .row > :nth-child(2) {
        grid-area: kind;
        color: var(--muted, #8b9bad);
        font-size: 11px;
      }
      .row > :nth-child(3) { display: none; }
      .row > :nth-child(4) {
        grid-area: modified;
        color: var(--muted, #8b9bad);
        font-size: 11px;
      }
      .btn-label { display: none; }
      .fab-action { justify-content: center; }
      .tools button:has(.btn-label),
      .more:has(.btn-label),
      .preview button:has(.btn-label),
      .confirm button:has(.btn-label),
      .trash button:has(.btn-label),
      .fab-action:has(.btn-label) {
        width: 44px;
        height: 44px;
        padding: 0;
        justify-content: center;
      }
      .layout.preview-open { grid-template-columns: minmax(0, 1fr); }
      .layout.preview-open .listing { display: none; }
      .preview { max-height: none; height: var(--pane-height); }
    }
    @media (pointer: coarse) {
      .crumb, .tools button, .more, .preview button, .entry { min-height: 44px; }
    }
  `
}
