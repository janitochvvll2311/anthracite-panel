import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { live } from 'lit/directives/live.js'
import { localized, msg } from '@lit/localize'

import { formatBytes } from './format'
import { encodeBase64 } from './base64'
import { describeError } from './errors'
import { openFileEditor } from './editor-window'
import { FILES_CHANGED_EVENT } from './file-editor'
import type { FileChunk, FileEntry, FilePage, FilesClient } from './files-client'
import { join, normalize, parent } from './paths'
import './fab-menu'
import './icon'

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
  private confirmingDelete = false

  @state()
  private error = ''

  @state()
  private busy = false

  @state()
  private editingPath = false

  @state()
  private pathDraft = ''

  override connectedCallback(): void {
    super.connectedCallback()
    document.addEventListener(FILES_CHANGED_EVENT, this.onFilesChanged)
    queueMicrotask(() => void this.open(this.path))
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
    this.busy = true
    try {
      const page: FilePage = await this.client.list(normalize(path))
      this.path = normalize(path)
      this.entries = page.items
      this.next = page.next ?? null
      this.previewEntry = undefined
      this.chunk = undefined
      this.previewPath = ''
      this.previewUnavailable = false
      this.confirmingDelete = false
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
    this.previewEntry = entry
    this.previewPath = path
    this.chunk = undefined
    this.previewUnavailable = false
    this.confirmingDelete = false
    this.error = ''
    this.busy = true
    try {
      this.chunk = await this.client.read(path)
    } catch {
      // Binary files, permissions and transport errors all land on the same
      // generic placeholder; the details and the actions still work.
      this.previewUnavailable = true
    } finally {
      this.busy = false
    }
  }

  private closePreview(): void {
    this.previewEntry = undefined
    this.chunk = undefined
    this.previewPath = ''
    this.previewUnavailable = false
    this.confirmingDelete = false
  }

  private async deleteFile(): Promise<void> {
    if (!this.client || !this.previewPath) {
      return
    }
    this.busy = true
    try {
      await this.client.delete(this.previewPath)
      const directory = this.path
      this.closePreview()
      await this.open(directory)
      this.error = ''
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

  private pickUpload(): void {
    this.shadowRoot?.querySelector<HTMLInputElement>('.picker')?.click()
  }

  private async onUpload(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement
    const file = input.files?.[0]
    input.value = ''
    if (!file || !this.client) {
      return
    }
    this.busy = true
    try {
      const bytes = new Uint8Array(await file.arrayBuffer())
      await this.client.upload(join(this.path, file.name), encodeBase64(bytes), true)
      await this.open(this.path)
      this.error = ''
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
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
                <anth-icon
                  name=${entry.kind === 'dir' ? 'files' : entry.kind === 'symlink' ? 'link' : 'file'}
                ></anth-icon>
                <span>${entry.name}${entry.kind === 'dir' ? '/' : ''}</span>
              </button>
              <span>${this.kindLabel(entry.kind)}</span>
              <span class="num">${entry.kind === 'file' ? formatBytes(entry.size) : '-'}</span>
              <span class="num">
                ${entry.modified_ms > 0 ? new Date(entry.modified_ms).toLocaleString() : '-'}
              </span>
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
    return html`
      <aside class="preview">
        <header>
          <span class="title" title=${this.previewPath}>${entry.name}</span>
          <div class="actions">
            ${entry.kind === 'file'
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
                <span>${msg('Delete this file?')}</span>
                <button
                  type="button"
                  class="danger"
                  aria-label=${msg('Delete')}
                  ?disabled=${this.busy}
                  @click=${() => void this.deleteFile()}
                >
                  <anth-icon name="trash"></anth-icon>
                  <span class="btn-label">${msg('Delete')}</span>
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
            <dd>${entry.kind === 'file' ? formatBytes(entry.size) : '-'}</dd>
          </div>
          <div>
            <dt>${msg('Modified')}</dt>
            <dd>
              ${entry.modified_ms > 0 ? new Date(entry.modified_ms).toLocaleString() : '-'}
            </dd>
          </div>
        </dl>
        <div class="content">
          ${this.chunk
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

  override render() {
    return html`
      <section class="files">
        <header class="bar">
          ${this.renderPath()}
          <div class="tools">
            <button
              type="button"
              aria-label=${msg('Up')}
              ?disabled=${this.busy || this.path === '/'}
              @click=${() => this.up()}
            >
              <anth-icon name="up"></anth-icon>
              <span class="btn-label">${msg('Up')}</span>
            </button>
            <button
              type="button"
              aria-label=${msg('Reload')}
              ?disabled=${this.busy}
              @click=${() => this.open(this.path)}
            >
              <anth-icon name="refresh"></anth-icon>
              <span class="btn-label">${msg('Reload')}</span>
            </button>
          </div>
        </header>

        ${this.error ? html`<div class="error" role="alert">${this.error}</div>` : nothing}

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
        <input class="picker" type="file" hidden @change=${this.onUpload} />
        <anth-fab-menu icon="plus" label=${msg('File actions')}>
          <button type="button" class="fab-action" aria-label=${msg('New file')} @click=${() => this.create()}>
            <anth-icon name="file-plus"></anth-icon>
            <span class="btn-label">${msg('New file')}</span>
          </button>
          <button
            type="button"
            class="fab-action"
            aria-label=${msg('Upload from device')}
            @click=${() => this.pickUpload()}
          >
            <anth-icon name="upload"></anth-icon>
            <span class="btn-label">${msg('Upload from device')}</span>
          </button>
        </anth-fab-menu>
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
    .crumb, .tools button, .more, .preview button, .confirm button {
      border: 1px solid transparent;
      border-radius: 6px;
      background: transparent;
      color: var(--text, #dbe6f2);
      padding: 4px 7px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .crumb:hover, .tools button:hover, .more:hover, .preview button:hover, .confirm button:hover {
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
    .tools button, .more, .preview button, .confirm button, .fab-action {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      border-color: var(--panel-border, #223041);
      background: rgba(125, 211, 252, 0.08);
    }
    .fab-action { min-height: 38px; }
    .picker { display: none; }
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
    .preview .danger { border-color: rgba(248, 113, 113, 0.4); color: var(--err, #f87171); }
    .preview .danger:hover { background: rgba(248, 113, 113, 0.12); }
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
