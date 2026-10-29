import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { live } from 'lit/directives/live.js'
import { localized, msg, str } from '@lit/localize'

import { createCodeEditor } from './code-editor'
import type { CodeEditor } from './code-editor'
import { describeError } from 'anth-wire'
import type { FilesClient } from 'anth-wire'
import { join } from 'anth-wire'
import './icon'

/** Fired on `document` after an editor writes a file. */
export const FILES_CHANGED_EVENT = 'anth-files-changed'

/** Fired when a create tab's title follows the typed name. */
export const EDITOR_TITLE_EVENT = 'anth-file-editor-title'

/** Fired after a save so the tab host can close that tab. */
export const EDITOR_DONE_EVENT = 'anth-file-editor-done'

/** Editors refuse to load or save more than this many bytes. */
const MAX_EDIT_BYTES = 4 * 1024 * 1024

/**
 * `anth-file-editor`: one open file inside the editor tabs.
 *
 * It owns its content, CodeMirror surface, cursor state and save flow, so an
 * editor keeps its unsaved text while another tab is active and while the
 * Files panel is parked.
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

  /** Whether the CodeMirror surface replaced the textarea. */
  @state()
  private engine: 'plain' | 'codemirror' = 'plain'

  @state()
  private line = 1

  @state()
  private column = 1

  @state()
  private selected = 0

  @state()
  private languageName = ''

  private codeEditor?: CodeEditor

  private mounting = false

  private tried = false

  override connectedCallback(): void {
    super.connectedCallback()
    this.languageName = msg('Plain text')
    if (this.mode === 'edit') {
      void this.load()
    } else {
      this.loaded = true
    }
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.codeEditor?.destroy()
    this.codeEditor = undefined
  }

  override firstUpdated(): void {
    void this.ensureCodeMirror()
    this.focusField()
  }

  override updated(): void {
    this.codeEditor?.setText(this.text)
  }

  /** Puts the cursor in the name, CodeMirror or textarea field. */
  focusField(): void {
    if (this.codeEditor) {
      this.codeEditor.focus()
      return
    }
    this.shadowRoot?.querySelector<HTMLElement>('input, textarea')?.focus()
  }

  /**
   * Mounts CodeMirror once the file is loaded. A failed import or mount leaves
   * the plain textarea in place, so the editor keeps working without it.
   */
  private async ensureCodeMirror(): Promise<void> {
    if (this.codeEditor || this.mounting || this.tried || !this.loaded || this.blocked) {
      return
    }
    this.mounting = true
    try {
      this.engine = 'codemirror'
      await this.updateComplete
      const host = this.renderRoot.querySelector<HTMLElement>('.codemirror')
      if (!host || !this.isConnected) {
        this.engine = 'plain'
        return
      }
      this.codeEditor = (await createCodeEditor(host, {
        doc: this.text,
        name: this.name,
        ariaLabel: msg('Content'),
        callbacks: {
          onChange: (text) => {
            this.text = text
          },
          onCursor: (line, column, selected) => {
            this.line = line
            this.column = column
            this.selected = selected
          },
          onLanguage: (name) => {
            this.languageName = name
          },
        },
      })) ?? undefined
      if (!this.codeEditor) {
        this.engine = 'plain'
        return
      }
      this.focusField()
    } catch {
      this.engine = 'plain'
    } finally {
      this.tried = true
      this.mounting = false
    }
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
      this.line = 1
      this.column = 1
      this.selected = 0
    } catch (error) {
      this.error = describeError(error)
      this.blocked = true
    } finally {
      this.loaded = true
      this.busy = false
      void this.updateComplete.then(() => {
        if (!this.blocked) {
          void this.ensureCodeMirror()
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
    this.codeEditor?.setLanguage(this.name)
  }

  private updateText(event: Event): void {
    const area = event.target as HTMLTextAreaElement
    this.text = area.value
    this.syncTextareaCursor(area)
  }

  private onTextareaCursor(event: Event): void {
    this.syncTextareaCursor(event.target as HTMLTextAreaElement)
  }

  private syncTextareaCursor(area: HTMLTextAreaElement): void {
    const before = area.value.slice(0, area.selectionStart)
    this.line = before.split('\n').length
    this.column = area.selectionStart - before.lastIndexOf('\n')
    this.selected = Math.abs(area.selectionEnd - area.selectionStart)
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
      this.dispatchEvent(
        new CustomEvent(EDITOR_DONE_EVENT, {
          bubbles: true,
          composed: true,
          detail: { path },
        }),
      )
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private onKeydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault()
      void this.save()
    }
  }

  private lineEnding(): string {
    return this.text.includes('\r\n') ? 'CRLF' : 'LF'
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
        <div class="surface">
          <div class="codemirror" ?hidden=${this.engine !== 'codemirror'}></div>
          <textarea
            .value=${live(this.text)}
            aria-label=${msg('Content')}
            spellcheck="false"
            ?hidden=${this.engine === 'codemirror'}
            ?disabled=${!this.loaded || this.blocked}
            @input=${this.updateText}
            @keyup=${this.onTextareaCursor}
            @click=${this.onTextareaCursor}
            @select=${this.onTextareaCursor}
            @focus=${this.onTextareaCursor}
          ></textarea>
        </div>
        <footer>
          <span class="target">${target}</span>
          <button
            type="button"
            class="save"
            aria-label=${msg('Save')}
            ?disabled=${this.busy ||
            !this.loaded ||
            this.blocked ||
            (this.mode === 'create' && this.name.trim() === '')}
            @click=${() => void this.save()}
          >
            <anth-icon name="save"></anth-icon>
            <span class="btn-label">${msg('Save')}</span>
          </button>
        </footer>
        <div class="statusbar">
          <span>${msg(str`Ln ${this.line}, Col ${this.column}`)}</span>
          ${this.selected > 0 ? html`<span>${msg(str`Sel ${this.selected}`)}</span>` : nothing}
          <span class="spacer"></span>
          <span>${this.languageName}</span>
          <span>UTF-8</span>
          <span>${this.lineEnding()}</span>
        </div>
      </div>
    `
  }

  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      flex: 1 1 auto;
      min-width: 0;
      min-height: 0;
      color: var(--text, #dbe6f2);
      font: 14px/1.5 var(--sans, system-ui, sans-serif);
    }
    .editor {
      display: flex;
      flex-direction: column;
      flex: 1 1 auto;
      gap: 8px;
      min-width: 0;
      min-height: 0;
    }
    [hidden] { display: none !important; }
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
    .surface {
      display: flex;
      flex: 1 1 0;
      min-width: 0;
      min-height: 0;
    }
    .codemirror {
      position: relative;
      display: flex;
      flex: 1 1 0;
      min-width: 0;
      min-height: 0;
      overflow: hidden;
    }
    .codemirror .cm-editor {
      width: 100%;
      height: 100%;
      min-width: 0;
      min-height: 0;
    }
    .codemirror .cm-scroller {
      overflow: auto;
      overscroll-behavior: contain;
    }
    textarea {
      flex: 1 1 0;
      min-width: 0;
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
      display: inline-flex;
      align-items: center;
      gap: 6px;
      border: 1px solid var(--accent, #7dd3fc);
      border-radius: 6px;
      background: rgba(125, 211, 252, 0.16);
      color: var(--text, #dbe6f2);
      padding: 4px 10px;
      font: 600 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .save anth-icon { font-size: 14px; }
    .save:hover:not(:disabled) { background: rgba(125, 211, 252, 0.26); }
    .save:disabled { opacity: 0.5; cursor: default; }
    .save:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    .statusbar {
      display: flex;
      flex: 0 0 auto;
      flex-wrap: wrap;
      align-items: center;
      gap: 0 10px;
      padding-top: 5px;
      border-top: 1px solid var(--panel-border, #223041);
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
    }
    .statusbar .spacer { flex: 1 1 auto; }
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
      .btn-label { display: none; }
      .save:has(.btn-label) {
        width: 44px;
        height: 44px;
        padding: 0;
        justify-content: center;
      }
    }
    @media (pointer: coarse) {
      .save { min-height: 44px; }
    }
  `
}

function byteLength(text: string): number {
  return new TextEncoder().encode(text).length
}
