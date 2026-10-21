import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import type { FileClient, FileEntry, FileListing } from '../lib/files/file-client'
import {
  breadcrumbs,
  fileKindLabel,
  formatBytes,
  formatModified,
  joinPath,
  normalizePath,
  parentPath,
  sortFileEntries,
  type FileSortDirection,
  type FileSortKey,
} from '../lib/files/display'
import { confirmAction, promptText } from '../lib/ui/dialog'
import { isEditable } from '../lib/editor/language'
import './anth-file-editor'

type PreviewKind = 'text' | 'image' | 'binary'

interface FilePreview {
  entry: FileEntry
  kind: PreviewKind
  text?: string
  url?: string
  mime: string
  size: number
  truncated: boolean
}

/**
 * Filesystem manager.
 *
 * The component owns navigation, sorting, hidden-file filtering and write
 * confirmations. It depends on the injected `FileClient`, not on `/api`.
 */
@customElement('anth-file-manager')
export class AnthFileManager extends LitElement {
  @property({ attribute: false })
  client?: FileClient

  @property({ type: String })
  path = '/'

  @property({ type: Boolean, attribute: 'auto-load' })
  autoLoad = true

  @property({ type: Boolean, attribute: 'show-hidden' })
  showHidden = false

  @state()
  private listing?: FileListing

  @state()
  private loading = false

  @state()
  private error = ''

  @state()
  private selected?: FileEntry

  @state()
  private preview?: FilePreview

  @state()
  private previewLoading = false

  @state()
  private sortKey: FileSortKey = 'name'

  @state()
  private sortDirection: FileSortDirection = 'asc'

  @state()
  private busy = false

  /** The path open in the editor, if any. Editing replaces the preview. */
  @state()
  private editing = false


  private generation = 0
  private previewGeneration = 0

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.generation += 1
    this.previewGeneration += 1
    this.revokePreviewUrl()
  }

  protected override willUpdate(changed: PropertyValues<this>): void {
    if (
      this.client &&
      this.autoLoad &&
      (changed.has('client') ||
        changed.has('path') ||
        (changed.has('autoLoad') && this.autoLoad))
    ) {
      void this.load()
    }
  }

  /** Fetches the current directory. */
  async load(): Promise<void> {
    // Do not refresh the listing while a mutation is still running; the
    // post-mutation load picks up the final state.
    if (!this.client || this.busy) return

    const generation = ++this.generation
    this.loading = true
    this.error = ''
    this.selected = undefined
    this.clearPreview()

    try {
      const listing = await this.client.list(this.path)
      if (generation !== this.generation) return
      this.listing = listing
    } catch (error) {
      if (generation !== this.generation) return
      this.error = messageOf(error)
    } finally {
      if (generation === this.generation) this.loading = false
    }
  }

  /** Re-fetches the current directory without changing the path. */
  reload(): void {
    void this.load()
  }

  /** Navigates to a virtual absolute path. */
  navigate(path: string): void {
    const next = normalizePath(path)
    if (next === this.path) {
      void this.load()
    } else {
      this.path = next
    }
  }

  private visibleEntries(): FileEntry[] {
    const entries = (this.listing?.entries ?? []).filter(
      (entry) => this.showHidden || !entry.hidden,
    )
    return sortFileEntries(entries, this.sortKey, this.sortDirection)
  }

  private toggleSort(key: FileSortKey): void {
    if (this.sortKey === key) {
      this.sortDirection = this.sortDirection === 'asc' ? 'desc' : 'asc'
      return
    }
    this.sortKey = key
    this.sortDirection = key === 'size' || key === 'modified' ? 'desc' : 'asc'
  }

  private select(entry: FileEntry): void {
    this.selected = entry
    if (entry.kind === 'directory') {
      this.clearPreview()
      return
    }
    void this.loadPreview(entry)
  }

  private open(entry: FileEntry): void {
    if (entry.kind === 'directory') {
      this.navigate(entry.path)
    } else {
      this.select(entry)
    }
  }

  private onRowKeydown(event: KeyboardEvent, entry: FileEntry): void {
    if (event.key === 'Enter') {
      event.preventDefault()
      this.open(entry)
    } else if (event.key === ' ') {
      event.preventDefault()
      this.select(entry)
    }
  }

  private async loadPreview(entry: FileEntry): Promise<void> {
    if (!this.client) return

    const generation = ++this.previewGeneration
    this.previewLoading = true
    this.error = ''

    try {
      const result = await this.client.read(entry.path)
      if (generation !== this.previewGeneration) return

      const mime = result.mime || result.blob.type || ''
      let kind: PreviewKind = 'binary'
      let text: string | undefined
      let url: string | undefined

      if (isSafeImageMime(mime)) {
        kind = 'image'
        url = URL.createObjectURL(result.blob)
      } else {
        const sample = new Uint8Array(
          await result.blob.slice(0, 512).arrayBuffer(),
        )
        if (!sample.includes(0)) {
          try {
            text = await result.blob.text()
            kind = 'text'
          } catch {
            kind = 'binary'
          }
        }
      }

      this.revokePreviewUrl()
      this.preview = {
        entry,
        kind,
        text,
        url,
        mime,
        size: result.size,
        truncated: result.truncated,
      }
    } catch (error) {
      if (generation !== this.previewGeneration) return
      this.error = messageOf(error)
      this.clearPreview()
    } finally {
      if (generation === this.previewGeneration) this.previewLoading = false
    }
  }

  private clearPreview(): void {
    this.previewGeneration += 1
    this.revokePreviewUrl()
    this.preview = undefined
    this.previewLoading = false
    this.editing = false
  }

  /**
   * Saves through the client, passing the mtime the preview was read at.
   *
   * The daemon refuses when the file changed since, and the editor keeps the
   * buffer dirty; here that means the message is surfaced and the preview is
   * not silently refreshed over the user's text.
   */
  private async saveEdit(contents: string): Promise<{ modified_ms: number }> {
    const preview = this.preview
    if (!preview || !this.client) throw new Error('nothing to save')

    const result = await this.client.writeText(
      preview.entry.path,
      contents,
      preview.entry.modified_ms,
    )
    // The saved text is now the file, so the preview can show it without a
    // second round trip.
    this.preview = {
      ...preview,
      text: contents,
      size: result.size,
      truncated: false,
      entry: { ...preview.entry, size: result.size, modified_ms: result.modified_ms },
    }
    return result
  }

  private handleEditorKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      // Leaving the editor discards nothing: the text is still in the editor's
      // history, but the preview is what the file on disk says.
      this.editing = false
    }
  }

  private revokePreviewUrl(): void {
    if (this.preview?.url) URL.revokeObjectURL(this.preview.url)
  }

  private async mutate(action: () => Promise<unknown>): Promise<void> {
    if (!this.client || this.busy) return

    this.busy = true
    this.error = ''
    let changed = false
    try {
      await action()
      changed = true
    } catch (error) {
      this.error = messageOf(error)
    } finally {
      this.busy = false
    }

    if (changed) await this.load()
  }

  private async newFolder(): Promise<void> {
    const name = await promptText({
      title: 'New folder',
      label: 'Name',
      confirmLabel: 'Create',
      validate: folderNameProblem,
    })
    if (name === undefined) return
    void this.mutate(() => this.client!.mkdir(joinPath(this.path, name.trim())))
  }

  private async renameSelected(): Promise<void> {
    const selected = this.selected
    if (!selected || !this.client) return
    const name = await promptText({
      title: `Rename ${selected.name}`,
      label: 'New name',
      value: selected.name,
      confirmLabel: 'Rename',
      validate: folderNameProblem,
    })
    if (name === undefined) return
    const trimmed = name.trim()
    if (trimmed === selected.name) return
    const parent = parentPath(selected.path) ?? '/'
    void this.mutate(() =>
      this.client!.rename(selected.path, joinPath(parent, trimmed)),
    )
  }

  /**
   * Deletes the selection, through the trash when the daemon offers it.
   *
   * A permanent delete of a directory is unrecoverable, so the panel only does
   * it when trashing is off, and the prompt says which one is about to happen.
   */
  private async deleteSelected(): Promise<void> {
    const selected = this.selected
    if (!selected || !this.client) return
    const trashable = this.listing?.trash_enabled === true
    const accepted = await confirmAction({
      title: trashable
        ? `Move ${selected.name} to the trash?`
        : `Delete ${selected.name}?`,
      message: trashable
        ? `${selected.path} can be restored from the trash.`
        : selected.kind === 'directory'
          ? `${selected.path} and everything inside it is removed permanently.`
          : `${selected.path} is removed permanently.`,
      confirmLabel: trashable ? 'Move to trash' : 'Delete',
      danger: !trashable,
    })
    if (!accepted) return
    void this.mutate(() =>
      trashable
        ? this.client!.trash(selected.path)
        : this.client!.delete(selected.path, selected.kind === 'directory'),
    )
  }

  private async onUpload(event: Event): Promise<void> {
    const input = event.currentTarget as HTMLInputElement
    const file = input.files?.[0]
    input.value = ''
    if (!file || !this.client) return

    const exists = this.listing?.entries.some((entry) => entry.name === file.name)
    if (
      exists &&
      !(await confirmAction({
        title: `Replace ${file.name}?`,
        message: 'The existing file is overwritten.',
        confirmLabel: 'Replace',
        danger: true,
      }))
    ) {
      return
    }

    void this.mutate(() =>
      this.client!.upload(joinPath(this.path, file.name), file, exists),
    )
  }

  private sortMark(key: FileSortKey): string {
    if (this.sortKey !== key) return '↕'
    return this.sortDirection === 'asc' ? '↑' : '↓'
  }

  private renderHeader(key: FileSortKey, label: string): unknown {
    return html`<th aria-sort=${this.ariaSortFor(key)}>
      <button
        type="button"
        class="sort"
        @click=${() => this.toggleSort(key)}
      >
        ${label} <span class="mark">${this.sortMark(key)}</span>
      </button>
    </th>`
  }

  private ariaSortFor(key: FileSortKey): 'ascending' | 'descending' | 'none' {
    if (this.sortKey !== key) return 'none'
    return this.sortDirection === 'asc' ? 'ascending' : 'descending'
  }

  private renderPreview(): unknown {
    if (this.previewLoading) {
      return html`<p class="muted">Loading preview…</p>`
    }
    if (!this.preview) {
      return html`<p class="muted">Select a file to preview it.</p>`
    }

    const preview = this.preview
    const link = this.client?.downloadUrl(preview.entry.path)
    return html`
      <header class="preview-head">
        <div>
          <strong>${preview.entry.name}</strong>
          <div class="muted">
            ${preview.mime || 'application/octet-stream'} ·
            ${formatBytes(preview.size)}
          </div>
        </div>
        <span class="spacer"></span>
        ${preview.kind === 'text' &&
        !preview.truncated &&
        isEditable(preview.entry.path) &&
        this.listing?.write_enabled === true
          ? html`<button
              type="button"
              class="edit-toggle"
              @click=${() => (this.editing = !this.editing)}
            >
              ${this.editing ? 'Close editor' : 'Edit'}
            </button>`
          : nothing}
        ${link
          ? html`<a class="download" href=${link} download>Download</a>`
          : nothing}
      </header>
      ${preview.truncated
        ? html`<p class="warning" role="status">
            Preview truncated; use Download for the full file. Editing is
            disabled because saving the preview would discard the rest.
          </p>`
        : nothing}
      ${this.editing
        ? html`
            <div class="editor-host" @keydown=${this.handleEditorKeydown}>
              <anth-file-editor
                .path=${preview.entry.path}
                .contents=${preview.text ?? ''}
                .writable=${this.listing?.write_enabled === true}
                .save=${(contents: string) => this.saveEdit(contents)}
              ></anth-file-editor>
            </div>
          `
        : preview.kind === 'text'
          ? html`<pre class="text-preview">${preview.text}</pre>`
          : preview.kind === 'image'
          ? html`<img
              class="image-preview"
              src=${preview.url}
              alt=${preview.entry.name}
            />`
          : html`<p class="muted">
              Binary file. Download it to inspect the contents.
            </p>`}
    `
  }

  override render() {
    const listing = this.listing
    const entries = this.visibleEntries()
    const current = normalizePath(this.path)
    const parent = parentPath(current)
    const writable = listing?.write_enabled === true

    return html`
      <section class="explorer" part="file-manager">
        <header class="toolbar">
          <nav class="crumbs" aria-label="Breadcrumb">
            ${breadcrumbs(current).map(
              (crumb, index) => html`
                ${index > 1 ? html`<span class="sep">/</span>` : nothing}
                <button
                  type="button"
                  class="crumb"
                  ?disabled=${crumb.path === current}
                  @click=${() => this.navigate(crumb.path)}
                >
                  ${crumb.name}
                </button>
              `,
            )}
          </nav>
          <div class="actions">
            <button
              type="button"
              ?disabled=${parent === null || this.loading}
              @click=${() => parent && this.navigate(parent)}
            >
              Up
            </button>
            <button
              type="button"
              ?disabled=${this.loading || this.busy}
              @click=${this.reload}
            >
              Refresh
            </button>
            <label class="hidden-toggle">
              <input
                type="checkbox"
                .checked=${this.showHidden}
                @change=${(event: Event) =>
                  (this.showHidden = (event.currentTarget as HTMLInputElement).checked)}
              />
              Hidden
            </label>
            ${writable
              ? html`
                  <button type="button" ?disabled=${this.busy} @click=${this.newFolder}>
                    New folder
                  </button>
                  <label class="upload-button">
                    Upload
                    <input type="file" hidden @change=${this.onUpload} />
                  </label>
                  <button
                    type="button"
                    ?disabled=${!this.selected || this.busy}
                    @click=${this.renameSelected}
                  >
                    Rename
                  </button>
                  <button
                    type="button"
                    class="danger"
                    ?disabled=${!this.selected || this.busy}
                    @click=${this.deleteSelected}
                  >
                    Delete
                  </button>
                `
              : html`<span class="read-only" part="read-only">Read-only</span>`}
          </div>
        </header>

        ${this.error
          ? html`<div class="error" role="alert">${this.error}</div>`
          : nothing}

        <div class="panes">
          <section class="list" part="list" aria-busy=${this.loading}>
            <table>
              <thead>
                <tr>
                  ${this.renderHeader('name', 'NAME')}
                  ${this.renderHeader('kind', 'TYPE')}
                  ${this.renderHeader('size', 'SIZE')}
                  ${this.renderHeader('modified', 'MODIFIED')}
                </tr>
              </thead>
              <tbody>
                ${entries.length === 0
                  ? html`<tr>
                      <td colspan="4" class="empty">
                        ${this.loading ? 'Loading…' : 'This directory is empty.'}
                      </td>
                    </tr>`
                  : entries.map((entry) => this.renderRow(entry))}
              </tbody>
            </table>
            ${listing?.warning
              ? html`<p class="warning" role="status">${listing.warning}</p>`
              : nothing}
          </section>

          <aside class="preview" part="preview">
            ${this.selected?.kind === 'directory' && !this.preview
              ? html`<p class="muted">
                  Directory: <code>${this.selected.path}</code>
                </p>`
              : this.renderPreview()}
          </aside>
        </div>
      </section>
    `
  }

  private renderRow(entry: FileEntry): unknown {
    const selected = this.selected?.path === entry.path
    return html`
      <tr
        class=${selected ? 'selected' : ''}
        tabindex="0"
        aria-selected=${selected ? 'true' : 'false'}
        @click=${() => this.select(entry)}
        @dblclick=${() => this.open(entry)}
        @keydown=${(event: KeyboardEvent) => this.onRowKeydown(event, entry)}
      >
        <td class="name">
          <span class="icon" aria-hidden="true"
            >${entry.kind === 'directory' ? '▸' : entry.kind === 'symlink' ? '↗' : '·'}</span
          >
          ${entry.name}${entry.kind === 'directory' ? '/' : ''}
        </td>
        <td>${fileKindLabel(entry.kind)}</td>
        <td class="number">
          ${entry.kind === 'directory' ? '—' : formatBytes(entry.size)}
        </td>
        <td>${formatModified(entry.modified_ms)}</td>
      </tr>
    `
  }

  static styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 14px/1.45 var(--sans, system-ui, sans-serif);
    }

    .explorer {
      display: flex;
      flex-direction: column;
      /* The list scrolls inside this height so the preview stays visible.
         Override --anth-file-height on the element to change the budget. */
      height: var(--anth-file-height, clamp(360px, 72vh, 760px));
      min-height: 0;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      overflow: hidden;
    }

    .toolbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      padding: 10px 12px;
      border-bottom: 1px solid var(--panel-border, #223041);
      background: color-mix(in srgb, var(--panel, #101721) 88%, black);
    }

    .crumbs {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 2px;
      min-width: 0;
      font-family: var(--mono, monospace);
    }

    .crumb {
      border: 0;
      background: transparent;
      color: var(--accent, #7dd3fc);
      padding: 2px 4px;
      font: inherit;
      cursor: pointer;
    }

    .crumb:disabled {
      color: var(--text, #dbe6f2);
      cursor: default;
    }

    .sep {
      color: var(--muted, #8b9bad);
    }

    .actions {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 6px;
    }

    button,
    .upload-button,
    .download {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      padding: 4px 9px;
      font: inherit;
      cursor: pointer;
    }

    button:hover:not(:disabled),
    .upload-button:hover,
    .download:hover {
      border-color: var(--accent, #7dd3fc);
    }

    button:disabled {
      opacity: 0.4;
      cursor: default;
    }

    button:focus-visible,
    .upload-button:focus-within,
    tr:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: -2px;
    }

    .danger {
      color: #fca5a5;
    }

    .upload-button {
      position: relative;
      display: inline-flex;
      align-items: center;
    }

    .hidden-toggle {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      color: var(--muted, #8b9bad);
    }

    .read-only {
      color: var(--muted, #8b9bad);
      border: 1px dashed var(--panel-border, #223041);
      border-radius: 999px;
      padding: 2px 8px;
    }

    .error,
    .warning {
      margin: 8px 12px;
      padding: 6px 9px;
      border-radius: 6px;
      background: rgba(248, 113, 113, 0.12);
      color: #fecaca;
    }

    .warning {
      background: rgba(251, 191, 36, 0.12);
      color: #fde68a;
    }

    .panes {
      display: grid;
      grid-template-columns: minmax(0, 1.35fr) minmax(220px, 1fr);
      grid-template-rows: minmax(0, 1fr);
      flex: 1;
      min-height: 0;
    }

    .list {
      min-width: 0;
      min-height: 0;
      overflow: auto;
      border-right: 1px solid var(--panel-border, #223041);
    }

    table {
      width: 100%;
      min-width: 40rem;
      table-layout: fixed;
      border-collapse: collapse;
      font-family: var(--mono, monospace);
      font-size: 13px;
      white-space: nowrap;
    }

    th {
      position: sticky;
      top: 0;
      z-index: 1;
      background: #0c131c;
      text-align: left;
      font-weight: 600;
      white-space: nowrap;
    }

    th,
    td {
      padding: 5px 9px;
      border-bottom: 1px solid rgba(34, 48, 65, 0.55);
    }

    td {
      overflow: hidden;
      text-overflow: ellipsis;
    }

    th:nth-child(2),
    td:nth-child(2) {
      width: 5.5rem;
    }

    th:nth-child(3),
    td:nth-child(3) {
      width: 6.5rem;
    }

    th:nth-child(4),
    td:nth-child(4) {
      width: 11.5rem;
    }

    th .sort {
      border: 0;
      background: transparent;
      padding: 0;
      color: inherit;
      font: inherit;
    }

    .mark {
      color: var(--muted, #8b9bad);
    }

    tbody tr {
      cursor: default;
    }

    tbody tr:hover {
      background: rgba(125, 211, 252, 0.06);
    }

    tbody tr.selected {
      background: rgba(125, 211, 252, 0.14);
    }

    td.name {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .icon {
      display: inline-block;
      width: 14px;
      color: var(--muted, #8b9bad);
    }

    .number {
      text-align: right;
      font-variant-numeric: tabular-nums;
    }

    .empty {
      padding: 24px;
      color: var(--muted, #8b9bad);
      text-align: center;
    }

    .preview {
      min-width: 0;
      min-height: 0;
      padding: 12px;
      overflow: auto;
    }

    .preview-head {
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 10px;
      margin-bottom: 8px;
    }

    .preview-head .spacer {
      flex: 1;
    }

    .edit-toggle {
      font: inherit;
      padding: 2px 8px;
      border-radius: 4px;
      border: 1px solid var(--border, #cfd8e3);
      background: var(--surface, #fff);
      cursor: pointer;
    }

    .editor-host {
      min-height: 18rem;
      height: 60vh;
      display: flex;
    }

    .editor-host anth-file-editor {
      flex: 1;
    }

    .preview-head strong {
      overflow-wrap: anywhere;
    }

    .muted {
      color: var(--muted, #8b9bad);
    }

    .text-preview {
      margin: 0;
      padding: 10px;
      max-height: 60vh;
      overflow: auto;
      border-radius: 6px;
      background: #070a0f;
      color: #dbe6f2;
      font-family: var(--mono, monospace);
      font-size: 12px;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
    }

    .image-preview {
      max-width: 100%;
      max-height: 60vh;
      border-radius: 6px;
      border: 1px solid var(--panel-border, #223041);
    }

    code {
      font-family: var(--mono, monospace);
      overflow-wrap: anywhere;
    }

    @media (max-width: 800px) {
      .panes {
        grid-template-columns: 1fr;
        grid-template-rows: minmax(0, 1.1fr) minmax(0, 1fr);
      }

      .list {
        border-right: 0;
        border-bottom: 1px solid var(--panel-border, #223041);
      }
    }
  `
}

function isSafeImageMime(mime: string): boolean {
  const normalized = mime.split(';', 1)[0].trim().toLowerCase()
  return [
    'image/png',
    'image/apng',
    'image/jpeg',
    'image/gif',
    'image/webp',
    'image/avif',
    'image/bmp',
    'image/x-icon',
    'image/vnd.microsoft.icon',
  ].includes(normalized)
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-file-manager': AnthFileManager
  }
}

/**
 * Rejects an empty name and one containing `/`, for both a new folder and a
 * rename. The dialog keeps the input open so the operator can fix it in place.
 */
function folderNameProblem(value: string): string | undefined {
  const trimmed = value.trim()
  if (!trimmed) return 'A name is required.'
  if (trimmed.includes('/')) return 'A name cannot contain `/`.'
  return undefined
}
