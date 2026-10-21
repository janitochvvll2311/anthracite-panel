/**
 * Text editor backed by CodeMirror.
 *
 * The component owns the header, the dirty flag and the save precondition;
 * CodeMirror owns the text, the line numbers and the highlighting. CodeMirror
 * is imported dynamically so the initial bundle does not pay for it, and the
 * component falls back to a plain textarea when that import fails.
 */

import { LitElement, html, css } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { ref } from 'lit/directives/ref.js'
import { languageFor } from '../lib/editor/language'
import type { EditorHandle } from '../lib/editor/codemirror-setup'

type SaveState = 'clean' | 'dirty' | 'saving' | 'saved' | 'error'

@customElement('anth-file-editor')
export class AnthFileEditor extends LitElement {
  /** Existing virtual path. The editor edits; creating files is `upload`. */
  @property({ type: String }) path = ''
  /** Contents as loaded; the parent owns loading and passes them in. */
  @property({ type: String }) contents = ''
  /** Save handler. Rejecting keeps the buffer dirty and shows the message. */
  @property({ attribute: false }) save:
    | ((contents: string) => Promise<{ modified_ms: number }>)
    | null = null
  /** When false the buffer is read-only and Save is disabled. */
  @property({ type: Boolean }) writable = true

  @state() private status: SaveState = 'clean'
  @state() private message = ''
  @state() private lineCount = 1

  private view: EditorHandle | null = null
  private host: HTMLDivElement | null = null
  private loading = false

  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      min-height: 0;
      border: 1px solid var(--anth-border, #d4d4d8);
      border-radius: 6px;
      background: var(--anth-surface, #fff);
      color: var(--anth-text, #18181b);
      font: inherit;
      overflow: hidden;
    }

    header {
      display: flex;
      align-items: center;
      gap: 0.5rem;
      padding: 0.4rem 0.6rem;
      border-bottom: 1px solid var(--anth-border, #d4d4d8);
      font-size: 0.8rem;
    }

    .path {
      flex: 1;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-family: ui-monospace, monospace;
    }

    .badge {
      padding: 0.1rem 0.4rem;
      border-radius: 999px;
      background: var(--anth-muted, #f4f4f5);
      font-size: 0.7rem;
      text-transform: uppercase;
      letter-spacing: 0.03em;
    }

    .badge[data-state='dirty'] {
      background: #fef3c7;
      color: #92400e;
    }

    .badge[data-state='error'] {
      background: #fee2e2;
      color: #991b1b;
    }

    .badge[data-state='saved'] {
      background: #dcfce7;
      color: #166534;
    }

    button {
      font: inherit;
      padding: 0.2rem 0.6rem;
      border-radius: 4px;
      border: 1px solid var(--anth-border, #d4d4d8);
      background: var(--anth-surface, #fff);
      cursor: pointer;
    }

    button:disabled {
      opacity: 0.5;
      cursor: default;
    }

    .editor {
      flex: 1;
      min-height: 12rem;
      overflow: hidden;
    }

    .fallback {
      width: 100%;
      height: 100%;
      min-height: 12rem;
      box-sizing: border-box;
      border: 0;
      padding: 0.5rem 0.6rem;
      resize: none;
      font-family: ui-monospace, monospace;
      font-size: 0.8rem;
      line-height: 1.5;
      tab-size: 2;
    }

    footer {
      display: flex;
      align-items: center;
      gap: 0.5rem;
      min-height: 1.6rem;
      padding: 0.2rem 0.6rem;
      border-top: 1px solid var(--anth-border, #d4d4d8);
      font-size: 0.75rem;
    }

    .error {
      color: #991b1b;
    }

    .spacer {
      flex: 1;
    }
  `

  override render() {
    return html`
      <header>
        <span class="path" title=${this.path}>${this.path || 'no file'}</span>
        <span class="badge" data-state=${this.status}>${this.status}</span>
        <button
          part="save"
          ?disabled=${!this.writable || this.status === 'saving' || !this.dirty}
          @click=${this.handleSave}
        >
          Save
        </button>
      </header>
      <div class="editor" ${ref(this.mountRef)}></div>
      <footer>
        <span class="error" role="alert">${this.message}</span>
        <span class="spacer"></span>
        <span>${this.lineCount} lines</span>
      </footer>
    `
  }

  private get dirty(): boolean {
    return this.status === 'dirty'
  }

  /** `ref` hands over the element after every render; mount at most once. */
  private mountRef = (element: Element | undefined) => {
    if (!(element instanceof HTMLDivElement) || element === this.host) {
      return
    }
    this.host = element
    void this.mount()
  }

  private async mount() {
    if (!this.host || !this.path || this.loading) {
      return
    }
    this.loading = true
    try {
      const { createEditor } = await import('../lib/editor/codemirror-setup')
      // The element can be removed while the modules load.
      if (!this.host) {
        return
      }
      this.view = await createEditor(
        this.host,
        this.contents,
        languageFor(this.path),
        () => {
          if (this.status === 'dirty') {
            return
          }
          this.status = 'dirty'
          this.message = ''
          this.lineCount = this.view?.lineCount() ?? this.lineCount
        },
      )
      this.lineCount = this.view.lineCount()
      this.view.focus()
    } catch (error) {
      // Without CodeMirror the editor still has to be usable, so the failure is
      // reported and the textarea fallback renders instead.
      this.message = error instanceof Error ? error.message : String(error)
      this.status = 'error'
      this.host.innerHTML = ''
      const textarea = document.createElement('textarea')
      textarea.className = 'fallback'
      textarea.spellcheck = false
      textarea.readOnly = !this.writable
      textarea.value = this.contents
      textarea.addEventListener('input', () => {
        this.status = 'dirty'
        this.lineCount = textarea.value.split('\n').length
      })
      this.host.append(textarea)
    } finally {
      this.loading = false
    }
  }

  override disconnectedCallback() {
    super.disconnectedCallback()
    this.view?.destroy()
    this.view = null
    this.host = null
  }

  private handleSave = async () => {
    if (!this.save) {
      return
    }
    const contents = this.currentValue
    this.status = 'saving'
    this.message = ''
    try {
      await this.save(contents)
      this.status = 'saved'
    } catch (error) {
      // A rejected save keeps the buffer dirty: failing to save does not mean
      // the text was saved.
      this.status = 'dirty'
      this.message = error instanceof Error ? error.message : String(error)
    }
  }

  /** The buffer, whether CodeMirror or the fallback textarea renders it. */
  get currentValue(): string {
    if (this.view) {
      return this.view.getValue()
    }
    const fallback = this.host?.querySelector('textarea')
    return fallback instanceof HTMLTextAreaElement ? fallback.value : this.contents
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-file-editor': AnthFileEditor
  }
}
