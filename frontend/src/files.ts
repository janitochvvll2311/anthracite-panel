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
  private chunk?: FileChunk

  @state()
  private previewPath = ''

  @state()
  private error = ''

  @state()
  private busy = false

  @state()
  private editor?: EditorState

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
      this.chunk = undefined
      this.previewPath = ''
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
    this.busy = true
    try {
      this.chunk = await this.client.read(path)
      this.previewPath = path
      this.error = ''
    } catch (error) {
      this.error = describe(error)
      this.chunk = undefined
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
    void this.updateComplete.then(() => {
      const field = this.shadowRoot?.querySelector<HTMLElement>('.editor input, .editor textarea')
      field?.focus()
    })
  }

  private closeEditor(): void {
    this.editor = undefined
  }

  private onEditorKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.stopPropagation()
      this.closeEditor()
    }
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
      <table>
        <thead>
          <tr>
            <th>${msg('Name')}</th>
            <th>${msg('Kind')}</th>
            <th class="num">${msg('Size')}</th>
            <th class="num">${msg('Modified')}</th>
            <th class="actions">${msg('Actions')}</th>
          </tr>
        </thead>
        <tbody>
          ${this.entries.map(
            (entry) => html`
              <tr>
                <td>
                  <button type="button" class="entry" @click=${() => this.activate(entry)}>
                    ${entry.name}${entry.kind === 'dir' ? '/' : ''}
                  </button>
                </td>
                <td>${this.kindLabel(entry.kind)}</td>
                <td class="num">${entry.kind === 'file' ? formatBytes(entry.size) : '-'}</td>
                <td class="num">
                  ${entry.modified_ms > 0 ? new Date(entry.modified_ms).toLocaleString() : '-'}
                </td>
                <td class="actions">
                  ${entry.kind === 'file'
                    ? html`<button
                        type="button"
                        class="action"
                        ?disabled=${this.busy}
                        @click=${() => this.edit(entry)}
                      >
                        ${msg('Edit')}
                      </button>`
                    : nothing}
                </td>
              </tr>
            `,
          )}
        </tbody>
      </table>
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
        <section class="editor">
          <header>
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

  private renderPreview(chunk: FileChunk) {
    return html`
      <aside class="preview">
        <header>
          <code>${this.previewPath}</code>
          <button type="button" @click=${() => (this.chunk = undefined)}>${msg('Close')}</button>
        </header>
        <pre>${chunk.text}</pre>
        ${chunk.eof
          ? nothing
          : html`<button type="button" ?disabled=${this.busy} @click=${() => this.morePreview()}>
              ${msg('Load more')}
            </button>`}
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

        <div class="layout ${this.chunk ? 'preview-open' : ''}">
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
          ${this.chunk ? this.renderPreview(this.chunk) : nothing}
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
    .files { position: relative; display: grid; gap: 12px; min-width: 0; }
    .files.editing { min-height: 24rem; }
    .bar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }
    .crumbs { display: flex; flex-wrap: wrap; align-items: center; gap: 2px; min-width: 0; }
    .crumb, .tools button, .more, .preview button {
      border: 1px solid transparent;
      border-radius: 6px;
      background: transparent;
      color: var(--text, #dbe6f2);
      padding: 4px 7px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .crumb:hover, .tools button:hover, .more:hover, .preview button:hover {
      border-color: var(--panel-border, #223041);
      background: rgba(125, 211, 252, 0.06);
    }
    .crumb[aria-current='page'] { color: var(--accent, #7dd3fc); }
    .sep { color: var(--muted, #8b9bad); }
    .tools { display: flex; gap: 6px; }
    .tools button, .more, .preview button {
      border-color: var(--panel-border, #223041);
      background: rgba(125, 211, 252, 0.08);
    }
    button:disabled { opacity: 0.5; cursor: default; }
    button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }

    .layout { display: grid; gap: 12px; min-width: 0; }
    .listing { min-width: 0; overflow-x: auto; }
    table { width: 100%; border-collapse: collapse; font: 13px/1.45 var(--mono, monospace); }
    th, td { padding: 6px 8px; text-align: left; border-bottom: 1px solid var(--panel-border, #223041); }
    th {
      color: var(--muted, #8b9bad);
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
    .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
    .entry {
      border: 0;
      padding: 0;
      background: transparent;
      color: var(--accent, #7dd3fc);
      font: inherit;
      text-align: left;
      cursor: pointer;
    }
    .entry:hover { text-decoration: underline; }
    .actions { width: 1%; text-align: right; white-space: nowrap; }
    .action {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: rgba(125, 211, 252, 0.08);
      color: var(--text, #dbe6f2);
      padding: 2px 8px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .action:hover { background: rgba(125, 211, 252, 0.16); }
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
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
      padding: 10px;
      min-width: 0;
    }
    .preview header { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
    .preview pre {
      margin: 8px 0 0;
      max-height: 24rem;
      overflow: auto;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
      font: 12px/1.5 var(--mono, monospace);
      color: var(--text, #dbe6f2);
    }
    .preview code { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
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
      max-height: 100%;
      padding: 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
    }
    .editor header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }
    .editor .title {
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
    .editor textarea { flex: 1 1 auto; min-height: 16rem; resize: vertical; }
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
      /* Below this width the preview and the editor take the panel over
         instead of sitting beside the listing. */
      .layout.preview-open { grid-template-columns: minmax(0, 1fr); }
      .layout.preview-open .listing { display: none; }
      .preview pre { max-height: none; }
      .files.editing { min-height: 0; }
      .files.editing .bar,
      .files.editing .layout,
      .files.editing > .error { display: none; }
      .editor-layer { position: static; padding: 0; background: none; }
      .editor { width: 100%; max-height: none; }
      .editor textarea { min-height: 55vh; resize: none; }
    }
    @media (pointer: coarse) {
      .crumb, .tools button, .more, .preview button, .entry, .action, .editor .close { min-height: 44px; }
    }
  `
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
