import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { live } from 'lit/directives/live.js'
import { localized, msg } from '@lit/localize'

import { formatBytes } from './format'
import type { FileChunk, FileEntry, FilePage, FilesClient } from './files-client'

interface EditorState {
  mode: 'edit' | 'create'
  /** Directory the editor was opened from. */
  directory: string
  /** Name inside that directory: selected on edit, typed on create. */
  name: string
  text: string
}

interface DragState {
  pointerId: number
  startX: number
  startY: number
  originX: number
  originY: number
}

/** A display path: always starts at `/` and has no empty or `.` segments. */
function normalize(path: string): string {
  const parts = path
    .split('/')
    .filter((part) => part !== '' && part !== '.')
  return `/${parts.join('/')}`
}

function join(base: string, name: string): string {
  return normalize(`${base}/${name}`)
}

function parent(path: string): string {
  const parts = path.split('/').filter(Boolean)
  parts.pop()
  return `/${parts.join('/')}`
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
  private confirmingDelete = false

  @state()
  private error = ''

  @state()
  private busy = false

  @state()
  private editor?: EditorState

  /** Pixel offset of the floating editor, relative to its centered position. */
  @state()
  private editorOffset = { x: 0, y: 0 }

  private drag?: DragState

  override connectedCallback(): void {
    super.connectedCallback()
    queueMicrotask(() => void this.open(this.path))
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
      this.editor = undefined
      this.error = ''
    } catch (error) {
      this.error = describe(error)
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
      this.error = describe(error)
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
      this.error = describe(error)
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
      this.error = describe(error)
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

  /** Reads one file and opens it in the editor. */
  private async edit(entry: FileEntry): Promise<void> {
    if (!this.client) {
      return
    }
    this.busy = true
    try {
      const chunk = await this.client.read(join(this.path, entry.name))
      this.showEditor({
        mode: 'edit',
        directory: this.path,
        name: entry.name,
        text: chunk.text,
      })
      this.error = ''
    } catch (error) {
      this.error = describe(error)
    } finally {
      this.busy = false
    }
  }

  private create(): void {
    this.showEditor({ mode: 'create', directory: this.path, name: '', text: '' })
    this.error = ''
  }

  private showEditor(editor: EditorState): void {
    this.editor = editor
    this.editorOffset = { x: 0, y: 0 }
    this.drag = undefined
    void this.updateComplete.then(() => {
      const field = this.shadowRoot?.querySelector<HTMLElement>('.editor input, .editor textarea')
      field?.focus()
    })
  }

  private closeEditor(): void {
    this.editor = undefined
    this.drag = undefined
  }

  private onEditorKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.stopPropagation()
      this.closeEditor()
    }
  }

  /** Starts a drag from the editor header on desktop; compact screens swap. */
  private startDrag(event: PointerEvent): void {
    if (event.button !== 0 || compact()) {
      return
    }
    const target = event.target as HTMLElement | null
    if (target?.closest('button, input, textarea, a')) {
      return
    }
    const handle = event.currentTarget as HTMLElement
    handle.setPointerCapture?.(event.pointerId)
    this.drag = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: this.editorOffset.x,
      originY: this.editorOffset.y,
    }
    event.preventDefault()
  }

  private onDrag(event: PointerEvent): void {
    const drag = this.drag
    if (!drag || drag.pointerId !== event.pointerId) {
      return
    }
    const layer = this.shadowRoot?.querySelector<HTMLElement>('.editor-layer')
    const editor = this.shadowRoot?.querySelector<HTMLElement>('.editor')
    if (!layer || !editor) {
      return
    }
    // The overlay centers the window; keep the whole window inside its box.
    const boundX = Math.max(0, (layer.clientWidth - editor.offsetWidth) / 2)
    const boundY = Math.max(0, (layer.clientHeight - editor.offsetHeight) / 2)
    this.editorOffset = {
      x: clamp(drag.originX + event.clientX - drag.startX, -boundX, boundX),
      y: clamp(drag.originY + event.clientY - drag.startY, -boundY, boundY),
    }
  }

  private endDrag(event: PointerEvent): void {
    if (!this.drag || this.drag.pointerId !== event.pointerId) {
      return
    }
    const handle = event.currentTarget as HTMLElement
    handle.releasePointerCapture?.(event.pointerId)
    this.drag = undefined
  }

  private setEditorName(event: Event): void {
    if (this.editor) {
      this.editor = { ...this.editor, name: (event.target as HTMLInputElement).value }
    }
  }

  private setEditorText(event: Event): void {
    if (this.editor) {
      this.editor = { ...this.editor, text: (event.target as HTMLTextAreaElement).value }
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

  private renderCrumbs() {
    const crumbs = this.crumbs()
    return html`
      <nav class="crumbs" aria-label=${msg('Path')}>
        ${crumbs.map(
          (crumb, index) => html`
            ${index > 1 ? html`<span class="sep" aria-hidden="true">/</span>` : nothing}
            <button
              type="button"
              class="crumb"
              aria-current=${crumb.path === this.path ? 'page' : nothing}
              ?disabled=${this.busy}
              @click=${() => this.open(crumb.path)}
            >
              ${crumb.label}
            </button>
          `,
        )}
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
                ${entry.name}${entry.kind === 'dir' ? '/' : ''}
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

  private renderEditor(editor: EditorState) {
    const target = editor.name ? join(editor.directory, editor.name) : editor.directory
    return html`
      <div
        class="editor-layer"
        role="dialog"
        aria-modal="true"
        aria-label=${msg('Editor')}
        @keydown=${this.onEditorKeydown}
      >
        <section
          class="editor"
          style="--editor-x: ${this.editorOffset.x}px; --editor-y: ${this.editorOffset.y}px"
        >
          <header
            @pointerdown=${this.startDrag}
            @pointermove=${this.onDrag}
            @pointerup=${this.endDrag}
            @pointercancel=${this.endDrag}
          >
            <span class="grip" aria-hidden="true">⠿</span>
            <span class="title">${editor.mode === 'create' ? msg('New file') : target}</span>
            <button type="button" class="close" @click=${() => this.closeEditor()}>
              ${msg('Close')}
            </button>
          </header>
          ${editor.mode === 'create'
            ? html`
                <label class="name">
                  ${msg('File name')}
                  <input
                    .value=${live(editor.name)}
                    placeholder="notes.txt"
                    autocomplete="off"
                    spellcheck="false"
                    @input=${this.setEditorName}
                  />
                </label>
              `
            : nothing}
          <textarea
            .value=${live(editor.text)}
            aria-label=${msg('Content')}
            spellcheck="false"
            @input=${this.setEditorText}
          ></textarea>
          <footer>
            <span class="target">${target}</span>
            <span class="hint">${msg('Saving arrives in a later update.')}</span>
          </footer>
        </section>
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
                  ?disabled=${this.busy}
                  @click=${() => void this.edit(entry)}
                >
                  ${msg('Edit')}
                </button>`
              : nothing}
            ${entry.kind === 'file' || entry.kind === 'symlink'
              ? html`<button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => (this.confirmingDelete = true)}
                >
                  ${msg('Delete')}
                </button>`
              : nothing}
            <button type="button" @click=${() => this.closePreview()}>${msg('Close')}</button>
          </div>
        </header>
        ${this.confirmingDelete
          ? html`
              <div class="confirm" role="alert">
                <span>${msg('Delete this file?')}</span>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${this.busy}
                  @click=${() => void this.deleteFile()}
                >
                  ${msg('Delete')}
                </button>
                <button
                  type="button"
                  ?disabled=${this.busy}
                  @click=${() => (this.confirmingDelete = false)}
                >
                  ${msg('Cancel')}
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
                      ?disabled=${this.busy}
                      @click=${() => this.morePreview()}
                    >
                      ${msg('Load more')}
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
      <section class="files ${this.editor ? 'editing' : ''}">
        <header class="bar">
          ${this.renderCrumbs()}
          <div class="tools">
            <button type="button" ?disabled=${this.busy} @click=${() => this.create()}>
              ${msg('New file')}
            </button>
            <button type="button" ?disabled=${this.busy || this.path === '/'} @click=${() => this.up()}>
              ${msg('Up')}
            </button>
            <button type="button" ?disabled=${this.busy} @click=${() => this.open(this.path)}>
              ${msg('Reload')}
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
              ? html`<button type="button" class="more" ?disabled=${this.busy} @click=${() => this.more()}>
                  ${msg('Load more')}
                </button>`
              : nothing}
          </div>
          ${this.renderPreview()}
        </div>
        ${this.editor ? this.renderEditor(this.editor) : nothing}
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
    .files.editing { min-height: 24rem; }
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
    .tools { display: flex; gap: 6px; }
    .tools button, .more, .preview button, .confirm button {
      border-color: var(--panel-border, #223041);
      background: rgba(125, 211, 252, 0.08);
    }
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
      border: 0;
      padding: 0;
      background: transparent;
      color: var(--accent, #7dd3fc);
      font: inherit;
      text-align: left;
      overflow-wrap: anywhere;
      cursor: pointer;
    }
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

    .editor-layer {
      position: absolute;
      inset: 0;
      z-index: 10;
      display: grid;
      place-items: center;
      padding: 12px;
      background: rgba(4, 8, 14, 0.62);
    }
    .editor {
      display: flex;
      flex-direction: column;
      gap: 8px;
      width: min(100%, 56rem);
      height: min(100%, 40rem);
      max-height: 100%;
      box-sizing: border-box;
      padding: 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
      transform: translate(var(--editor-x, 0px), var(--editor-y, 0px));
    }
    .editor header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      cursor: grab;
      user-select: none;
    }
    .editor header:active { cursor: grabbing; }
    .editor .grip { color: var(--muted, #8b9bad); font-size: 14px; line-height: 1; }
    .editor .title {
      flex: 1 1 auto;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font: 12px/1.4 var(--mono, monospace);
      color: var(--muted, #8b9bad);
    }
    .editor .close {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: rgba(125, 211, 252, 0.08);
      color: var(--text, #dbe6f2);
      padding: 4px 7px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .editor .close:hover { background: rgba(125, 211, 252, 0.16); }
    .editor .name {
      display: grid;
      gap: 4px;
      font: 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--muted, #8b9bad);
    }
    .editor input,
    .editor textarea {
      box-sizing: border-box;
      width: 100%;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: var(--panel-2, #0b1119);
      color: var(--text, #dbe6f2);
      padding: 8px 9px;
      font: 12px/1.5 var(--mono, monospace);
    }
    .editor textarea { flex: 1 1 auto; min-height: 0; resize: vertical; overscroll-behavior: contain; }
    .editor footer {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }
    .editor .target {
      min-width: 0;
      overflow-wrap: anywhere;
      font: 12px/1.4 var(--mono, monospace);
      color: var(--muted, #8b9bad);
    }
    .editor .hint { color: var(--warn, #fbbf24); font-size: 12px; }

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
      .layout.preview-open { grid-template-columns: minmax(0, 1fr); }
      .layout.preview-open .listing { display: none; }
      .preview { max-height: none; height: var(--pane-height); }
      .files.editing { min-height: 0; }
      .files.editing .bar,
      .files.editing .layout,
      .files.editing > .error { display: none; }
      .editor-layer { position: static; padding: 0; background: none; }
      .editor { width: 100%; height: var(--pane-height); max-height: none; transform: none; }
      .editor textarea { min-height: 0; resize: none; }
    }
    @media (pointer: coarse) {
      .crumb, .tools button, .more, .preview button, .entry, .editor .close { min-height: 44px; }
    }
  `
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

/** True when the shell uses its compact layout, where the editor swaps in. */
function compact(): boolean {
  return (
    typeof window.matchMedia === 'function' && window.matchMedia('(max-width: 48rem)').matches
  )
}
