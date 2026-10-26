import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { live } from 'lit/directives/live.js'
import { localized, msg } from '@lit/localize'

import type { FilesClient } from './files-client'
import { join } from './paths'
import { WINDOW_CLOSE_EVENT } from './window'
import type { AnthWindow } from './window'
import './window'

/** Fired on `document` after an editor writes a file. */
export const FILES_CHANGED_EVENT = 'anth-files-changed'

/** Fired when a create window's title follows the typed name. */
export const EDITOR_TITLE_EVENT = 'anth-file-editor-title'

/** Editors refuse to load or save more than this many bytes. */
const MAX_EDIT_BYTES = 4 * 1024 * 1024

/**
 * `anth-file-editor`: the content of a file-editor window.
 *
 * It owns its own read, write and dirty state, so it keeps working when the
 * Files panel that opened it is parked or unmounted. The window it lives in
 * is created by [`openFileEditor`] and outlives any route.
 */
@customElement('anth-file-editor')
@localized()
export class AnthFileEditor extends LitElement {
  @property({ attribute: false })
  client?: FilesClient

  @property()
  mode: 'edit' | 'create' = 'edit'

  @property()
  directory = '/'

  @property()
  name = ''

  @state()
  private text = ''

  @state()
  private error = ''

  @state()
  private busy = false

  @state()
  private loaded = false

  @state()
  private blocked = false

  override connectedCallback(): void {
    super.connectedCallback()
    if (this.mode === 'edit') {
      void this.load()
    } else {
      this.loaded = true
    }
  }

  override firstUpdated(): void {
    this.focusField()
  }

  /** Puts the cursor in the name or content field. */
  focusField(): void {
    this.shadowRoot?.querySelector<HTMLElement>('input, textarea')?.focus()
  }

  /** Reads every chunk, up to the editor cap, before allowing a save. */
  private async load(): Promise<void> {
    if (!this.client) {
      this.error = msg('No files client was configured')
      this.blocked = true
      return
    }
    this.busy = true
    try {
      const path = this.target()
      let chunk = await this.client.read(path)
      let text = chunk.text
      while (!chunk.eof) {
        if (text.length >= MAX_EDIT_BYTES) {
          this.blocked = true
          this.error = msg('This file is too large to edit.')
          break
        }
        chunk = await this.client.read(path, chunk.offset + byteLength(chunk.text))
        text += chunk.text
      }
      this.text = text
    } catch (error) {
      this.error = describe(error)
      this.blocked = true
    } finally {
      this.loaded = true
      this.busy = false
      void this.updateComplete.then(() => {
        if (!this.blocked) {
          this.focusField()
        }
      })
    }
  }

  private target(): string {
    return join(this.directory, this.name)
  }

  private rename(event: Event): void {
    this.name = (event.target as HTMLInputElement).value
    const title = this.name.trim() ? this.target() : msg('New file')
    this.dispatchEvent(
      new CustomEvent(EDITOR_TITLE_EVENT, {
        bubbles: true,
        composed: true,
        detail: { title },
      }),
    )
  }

  private updateText(event: Event): void {
    this.text = (event.target as HTMLTextAreaElement).value
  }

  private async save(): Promise<void> {
    if (!this.client || !this.loaded || this.blocked || this.busy) {
      return
    }
    if (this.mode === 'create' && this.name.trim() === '') {
      return
    }
    const path = this.target()
    this.busy = true
    this.error = ''
    try {
      await this.client.write(path, this.text, this.mode === 'create')
      this.dispatchEvent(
        new CustomEvent(FILES_CHANGED_EVENT, {
          bubbles: true,
          composed: true,
          detail: { directory: this.directory, path },
        }),
      )
      this.closeWindow()
    } catch (error) {
      this.error = describe(error)
    } finally {
      this.busy = false
    }
  }

  private closeWindow(): void {
    const window = this.closest('anth-window') as AnthWindow | null
    if (window) {
      window.close()
      return
    }
    this.remove()
  }

  private onKeydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault()
      void this.save()
    }
  }

  override render() {
    const target = this.target()
    return html`
      <div class="editor" @keydown=${this.onKeydown}>
        ${this.mode === 'create'
          ? html`
              <label class="name">
                ${msg('File name')}
                <input
                  .value=${live(this.name)}
                  placeholder="notes.txt"
                  autocomplete="off"
                  spellcheck="false"
                  @input=${this.rename}
                />
              </label>
            `
          : nothing}
        ${this.error ? html`<div class="error" role="alert">${this.error}</div>` : nothing}
        <textarea
          .value=${live(this.text)}
          aria-label=${msg('Content')}
          spellcheck="false"
          ?disabled=${!this.loaded || this.blocked}
          @input=${this.updateText}
        ></textarea>
        <footer>
          <span class="target">${target}</span>
          <button
            type="button"
            class="save"
            ?disabled=${this.busy ||
            !this.loaded ||
            this.blocked ||
            (this.mode === 'create' && this.name.trim() === '')}
            @click=${() => void this.save()}
          >
            ${msg('Save')}
          </button>
        </footer>
      </div>
    `
  }

  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      flex: 1 1 auto;
      min-height: 0;
      color: var(--text, #dbe6f2);
      font: 14px/1.5 var(--sans, system-ui, sans-serif);
    }
    .editor {
      display: flex;
      flex-direction: column;
      flex: 1 1 auto;
      gap: 8px;
      min-height: 0;
    }
    .name {
      display: grid;
      gap: 4px;
      font: 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--muted, #8b9bad);
    }
    input,
    textarea {
      box-sizing: border-box;
      width: 100%;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: var(--panel-2, #0b1119);
      color: var(--text, #dbe6f2);
      padding: 8px 9px;
      font: 12px/1.5 var(--mono, monospace);
    }
    textarea {
      flex: 1 1 auto;
      min-height: 0;
      resize: vertical;
      overscroll-behavior: contain;
    }
    textarea:disabled { opacity: 0.6; }
    footer {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }
    .target {
      min-width: 0;
      overflow-wrap: anywhere;
      font: 12px/1.4 var(--mono, monospace);
      color: var(--muted, #8b9bad);
    }
    .save {
      border: 1px solid var(--accent, #7dd3fc);
      border-radius: 6px;
      background: rgba(125, 211, 252, 0.16);
      color: var(--text, #dbe6f2);
      padding: 4px 10px;
      font: 600 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .save:hover:not(:disabled) { background: rgba(125, 211, 252, 0.26); }
    .save:disabled { opacity: 0.5; cursor: default; }
    .save:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    .error {
      margin: 0;
      padding: 0.5rem 0.65rem;
      border-radius: 8px;
      border: 1px solid rgba(248, 113, 113, 0.4);
      background: rgba(248, 113, 113, 0.12);
      color: var(--err, #f87171);
      font-size: 12px;
    }
    @media (max-width: 48rem) {
      textarea { resize: none; }
    }
    @media (pointer: coarse) {
      .save { min-height: 44px; }
    }
  `
}

export interface OpenFileEditor {
  client: FilesClient
  mode: 'edit' | 'create'
  directory: string
  name: string
}

const openByPath = new Map<string, AnthWindow>()

/**
 * Opens one editor window. Editing the same path again restores the existing
 * window instead of opening a duplicate; create windows are always new.
 */
export function openFileEditor(options: OpenFileEditor): AnthWindow {
  const path = options.mode === 'edit' ? join(options.directory, options.name) : ''
  const existing = path ? openByPath.get(path) : undefined
  if (existing?.isConnected) {
    existing.restore()
    existing.open = true
    const editor = existing.querySelector('anth-file-editor') as AnthFileEditor | null
    void editor?.updateComplete.then(() => editor.focusField())
    return existing
  }

  const editor = document.createElement('anth-file-editor') as AnthFileEditor
  editor.client = options.client
  editor.mode = options.mode
  editor.directory = options.directory
  editor.name = options.name

  const window = document.createElement('anth-window') as AnthWindow
  window.label = options.mode === 'create' ? msg('New file') : path
  window.append(editor)
  editor.addEventListener(EDITOR_TITLE_EVENT, (event) => {
    const detail = (event as CustomEvent<{ title?: string }>).detail
    if (detail?.title) {
      window.label = detail.title
    }
  })
  window.addEventListener(WINDOW_CLOSE_EVENT, () => {
    if (path) {
      openByPath.delete(path)
    }
    window.remove()
  })
  document.body.append(window)
  window.open = true
  void editor.updateComplete.then(() => editor.focusField())
  if (path) {
    openByPath.set(path, window)
  }
  return window
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function byteLength(text: string): number {
  return new TextEncoder().encode(text).length
}
