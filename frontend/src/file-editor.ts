import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { live } from 'lit/directives/live.js'
import { repeat } from 'lit/directives/repeat.js'
import { localized, msg, str } from '@lit/localize'

import type { EditorState as CodeMirrorState } from '@codemirror/state'
import type { EditorView as CodeMirrorView } from '@codemirror/view'

import type { FilesClient } from './files-client'
import { join } from './paths'
import { WINDOW_CLOSE_EVENT } from './window'
import type { AnthWindow } from './window'
import './window'

/** Fired on `document` after an editor writes a file. */
export const FILES_CHANGED_EVENT = 'anth-files-changed'

/** Fired when a create tab's title follows the typed name. */
export const EDITOR_TITLE_EVENT = 'anth-file-editor-title'

/** Fired after a save so the tab host can close that tab. */
export const EDITOR_DONE_EVENT = 'anth-file-editor-done'

/** Fired when the last tab closes, so the window goes away too. */
export const FILE_TABS_EMPTY_EVENT = 'anth-file-tabs-empty'

/** Editors refuse to load or save more than this many bytes. */
const MAX_EDIT_BYTES = 4 * 1024 * 1024

/**
 * CodeMirror needs selection and shadow-DOM APIs that the test DOM does not
 * emulate; the textarea fallback is what the unit tests exercise.
 */
const canUseCodeMirror =
  (import.meta as ImportMeta & { env?: { MODE?: string } }).env?.MODE !== 'test'

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

  private view?: CodeMirrorView

  private mounting = false

  private tried = false

  private reconfigureLanguage?: (name: string) => Promise<void>

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
    this.view?.destroy()
    this.view = undefined
  }

  override firstUpdated(): void {
    void this.ensureCodeMirror()
    this.focusField()
  }

  override updated(): void {
    if (!this.view) {
      return
    }
    const current = this.view.state.doc.toString()
    if (current !== this.text) {
      this.view.dispatch({ changes: { from: 0, to: current.length, insert: this.text } })
    }
  }

  /** Puts the cursor in the name, CodeMirror or textarea field. */
  focusField(): void {
    if (this.view) {
      this.view.requestMeasure()
      this.view.focus()
      return
    }
    this.shadowRoot?.querySelector<HTMLElement>('input, textarea')?.focus()
  }

  /**
   * Loads CodeMirror on demand. A failed import or mount leaves the plain
   * textarea in place, so the editor keeps working without the extra chunk.
   */
  private async ensureCodeMirror(): Promise<void> {
    if (!canUseCodeMirror) {
      this.tried = true
      return
    }
    if (this.view || this.mounting || this.tried || !this.loaded || this.blocked) {
      return
    }
    this.mounting = true
    try {
      const [
        { EditorView, basicSetup },
        { Compartment, EditorState },
        { HighlightStyle, LanguageDescription, syntaxHighlighting },
        { tags },
        { languages },
      ] = await Promise.all([
        import('codemirror'),
        import('@codemirror/state'),
        import('@codemirror/language'),
        import('@lezer/highlight'),
        import('@codemirror/language-data'),
      ])
      if (!this.isConnected) {
        return
      }
      this.engine = 'codemirror'
      await this.updateComplete
      if (!this.isConnected) {
        this.engine = 'plain'
        return
      }
      const host = this.renderRoot.querySelector<HTMLElement>('.codemirror')
      if (!host) {
        this.engine = 'plain'
        return
      }

      const highlight = HighlightStyle.define([
        { tag: tags.keyword, color: '#c792ea' },
        {
          tag: [
            tags.name,
            tags.deleted,
            tags.character,
            tags.propertyName,
            tags.macroName,
          ],
          color: '#dbe6f2',
        },
        { tag: [tags.function(tags.variableName), tags.labelName], color: '#7dd3fc' },
        {
          tag: [tags.color, tags.constant(tags.name), tags.standard(tags.name)],
          color: '#fbbf24',
        },
        { tag: [tags.definition(tags.name), tags.separator], color: '#dbe6f2' },
        {
          tag: [
            tags.typeName,
            tags.className,
            tags.number,
            tags.changed,
            tags.annotation,
            tags.modifier,
            tags.self,
            tags.namespace,
          ],
          color: '#f59e0b',
        },
        {
          tag: [tags.operator, tags.operatorKeyword, tags.url, tags.escape, tags.regexp],
          color: '#7dd3fc',
        },
        { tag: [tags.meta, tags.comment], color: '#8b9bad', fontStyle: 'italic' },
        { tag: tags.strong, fontWeight: 'bold' },
        { tag: tags.emphasis, fontStyle: 'italic' },
        { tag: tags.strikethrough, textDecoration: 'line-through' },
        { tag: tags.link, color: '#7dd3fc', textDecoration: 'underline' },
        { tag: [tags.atom, tags.bool, tags.special(tags.variableName)], color: '#f87171' },
        { tag: [tags.processingInstruction, tags.string, tags.inserted], color: '#86efac' },
        { tag: tags.invalid, color: '#f87171' },
      ])

      const compartment = new Compartment()
      const detect = async (name: string) => {
        const description = LanguageDescription.matchFilename(languages, name)
        if (!description) {
          return { support: null, name: msg('Plain text') }
        }
        try {
          return { support: await description.load(), name: description.name }
        } catch {
          return { support: null, name: msg('Plain text') }
        }
      }
      const initial = await detect(this.name)
      this.languageName = initial.name
      this.reconfigureLanguage = async (name: string) => {
        const detected = await detect(name)
        this.languageName = detected.name
        this.view?.dispatch({ effects: compartment.reconfigure(detected.support ?? []) })
      }

      const state: CodeMirrorState = EditorState.create({
        doc: this.text,
        extensions: [
          basicSetup,
          compartment.of(initial.support ?? []),
          syntaxHighlighting(highlight),
          EditorView.contentAttributes.of({
            'aria-label': msg('Content'),
            spellcheck: 'false',
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              this.text = update.state.doc.toString()
            }
            if (update.docChanged || update.selectionSet) {
              this.syncViewCursor(update.state)
            }
          }),
          EditorView.theme(
            {
              '&': {
                height: '100%',
                color: 'var(--text, #dbe6f2)',
                backgroundColor: 'transparent',
                fontSize: '12px',
              },
              '.cm-scroller': {
                fontFamily: 'var(--mono, monospace)',
                lineHeight: '1.5',
                overflow: 'auto',
              },
              '.cm-content': { caretColor: 'var(--accent, #7dd3fc)' },
              '.cm-gutters': {
                backgroundColor: 'transparent',
                color: 'var(--muted, #8b9bad)',
                border: 'none',
              },
              '.cm-activeLine': { backgroundColor: 'rgba(125, 211, 252, 0.06)' },
              '.cm-activeLineGutter': { backgroundColor: 'transparent' },
              '&.cm-focused': { outline: 'none' },
              '.cm-selectionBackground': { backgroundColor: 'rgba(125, 211, 252, 0.25)' },
              '&.cm-focused .cm-selectionBackground': {
                backgroundColor: 'rgba(125, 211, 252, 0.3)',
              },
              '.cm-cursor': { borderLeftColor: 'var(--accent, #7dd3fc)' },
            },
            { dark: true },
          ),
        ],
      })
      this.view = new EditorView({ state, parent: host })
      this.syncViewCursor(state)
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
      this.error = describe(error)
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
    void this.reconfigureLanguage?.(this.name)
  }

  private updateText(event: Event): void {
    const area = event.target as HTMLTextAreaElement
    this.text = area.value
    this.syncCursor(area.value, area.selectionStart, area.selectionEnd)
  }

  private onTextareaCursor(event: Event): void {
    const area = event.target as HTMLTextAreaElement
    this.syncCursor(area.value, area.selectionStart, area.selectionEnd)
  }

  private syncCursor(value: string, start: number, end: number): void {
    const before = value.slice(0, start)
    this.line = before.split('\n').length
    this.column = start - before.lastIndexOf('\n')
    this.selected = Math.abs(end - start)
  }

  private syncViewCursor(state: CodeMirrorState): void {
    const head = state.selection.main.head
    const line = state.doc.lineAt(head)
    this.line = line.number
    this.column = head - line.from + 1
    this.selected = Math.abs(state.selection.main.to - state.selection.main.from)
  }

  private lineEnding(): string {
    return this.text.includes('\r\n') ? 'CRLF' : 'LF'
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
      this.error = describe(error)
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
            ?disabled=${this.busy ||
            !this.loaded ||
            this.blocked ||
            (this.mode === 'create' && this.name.trim() === '')}
            @click=${() => void this.save()}
          >
            ${msg('Save')}
          </button>
        </footer>
        <div class="statusbar">
          <span>${msg(str`Ln ${this.line}, Col ${this.column}`)}</span>
          ${this.selected > 0
            ? html`<span>${msg(str`Sel ${this.selected}`)}</span>`
            : nothing}
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
      position: absolute;
      inset: 0;
      min-width: 0;
      min-height: 0;
      width: auto;
      height: auto;
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
    }
    @media (pointer: coarse) {
      .save { min-height: 44px; }
    }
  `
}

interface FileTab {
  key: string
  mode: 'edit' | 'create'
  directory: string
  name: string
  /** Full path on edit tabs; empty on create tabs until a name is typed. */
  path: string
  label: string
}

let tabIds = 0

/**
 * The tab strip and the editors of the single file-editor window. Every open
 * file keeps its own `anth-file-editor`, so switching tabs never reloads or
 * loses unsaved text.
 */
@customElement('anth-file-tabs')
@localized()
export class AnthFileTabs extends LitElement {
  @property({ attribute: false })
  client?: FilesClient

  @state()
  private tabs: FileTab[] = []

  @state()
  private active = ''

  /** Adds a tab, or activates the one already open for that path. */
  open(options: OpenFileEditor): string {
    this.client = options.client
    if (options.mode === 'edit') {
      const path = join(options.directory, options.name)
      const existing = this.tabs.find((tab) => tab.path === path)
      if (existing) {
        this.activate(existing.key)
        return existing.key
      }
    }
    const key = `tab-${++tabIds}`
    const path = options.mode === 'edit' ? join(options.directory, options.name) : ''
    const label = options.mode === 'edit' ? path : uniqueNewLabel(this.tabs)
    this.tabs = [
      ...this.tabs,
      {
        key,
        mode: options.mode,
        directory: options.directory,
        name: options.name,
        path,
        label,
      },
    ]
    this.active = key
    void this.updateComplete.then(() => this.focusEditor(key))
    return key
  }

  /** Puts the cursor in whatever tab is visible. */
  focusActive(): void {
    void this.updateComplete.then(() => this.focusEditor(this.active))
  }

  private activate(key: string): void {
    this.active = key
    void this.updateComplete.then(() => this.focusEditor(key))
  }

  private closeTab(key: string): void {
    if (!this.tabs.some((tab) => tab.key === key)) {
      return
    }
    const tabs = this.tabs.filter((tab) => tab.key !== key)
    this.tabs = tabs
    if (this.active === key) {
      this.active = tabs.at(-1)?.key ?? ''
    }
    if (tabs.length === 0) {
      this.dispatchEvent(
        new CustomEvent(FILE_TABS_EMPTY_EVENT, { bubbles: true, composed: true }),
      )
      return
    }
    void this.updateComplete.then(() => this.focusEditor(this.active))
  }

  private onTitle(event: Event): void {
    const key = (event.target as HTMLElement).dataset.key
    const title = (event as CustomEvent<{ title?: string }>).detail?.title
    if (!key || !title) {
      return
    }
    this.tabs = this.tabs.map((tab) => (tab.key === key ? { ...tab, label: title } : tab))
  }

  private onDone(event: Event): void {
    const key = (event.target as HTMLElement).dataset.key
    if (key) {
      this.closeTab(key)
    }
  }

  private focusEditor(key: string): void {
    if (!key) {
      return
    }
    const editor = this.shadowRoot?.querySelector<AnthFileEditor>(
      `anth-file-editor[data-key="${key}"]`,
    )
    editor?.focusField()
  }

  override render() {
    return html`
      <div class="tabs" role="tablist">
        ${this.tabs.map(
          (tab) => html`
            <div class="tab" data-active=${tab.key === this.active ? '' : nothing}>
              <button
                type="button"
                class="tab-button"
                role="tab"
                aria-selected=${tab.key === this.active}
                title=${tab.label}
                @click=${() => this.activate(tab.key)}
              >
                ${tab.label}
              </button>
              <button
                type="button"
                class="tab-close"
                title=${msg('Close')}
                aria-label=${msg('Close')}
                @click=${() => this.closeTab(tab.key)}
              >
                ✕
              </button>
            </div>
          `,
        )}
      </div>
      <div class="editors">
        ${repeat(
          this.tabs,
          (tab) => tab.key,
          (tab) => html`
            <anth-file-editor
              data-key=${tab.key}
              .client=${this.client}
              .mode=${tab.mode}
              .directory=${tab.directory}
              .name=${tab.name}
              ?hidden=${tab.key !== this.active}
              @anth-file-editor-title=${this.onTitle}
              @anth-file-editor-done=${this.onDone}
            ></anth-file-editor>
          `,
        )}
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
    }
    .tabs {
      display: flex;
      flex: 0 0 auto;
      gap: 4px;
      overflow-x: auto;
      overscroll-behavior-x: contain;
      -webkit-overflow-scrolling: touch;
      scrollbar-width: thin;
      padding-bottom: 4px;
      border-bottom: 1px solid var(--panel-border, #223041);
    }
    .tab {
      display: flex;
      flex: 0 0 auto;
      align-items: center;
      border: 1px solid var(--panel-border, #223041);
      border-bottom: 0;
      border-radius: 6px 6px 0 0;
      background: rgba(125, 211, 252, 0.04);
    }
    .tab[data-active] {
      border-color: var(--accent, #7dd3fc);
      background: rgba(125, 211, 252, 0.14);
    }
    .tab-button {
      max-width: 16rem;
      overflow: hidden;
      border: 0;
      background: transparent;
      color: var(--muted, #8b9bad);
      padding: 5px 8px;
      font: 12px/1.4 var(--mono, monospace);
      text-overflow: ellipsis;
      white-space: nowrap;
      cursor: pointer;
    }
    .tab[data-active] .tab-button { color: var(--text, #dbe6f2); }
    .tab-close {
      border: 0;
      border-left: 1px solid var(--panel-border, #223041);
      background: transparent;
      color: var(--muted, #8b9bad);
      padding: 5px 7px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .tab-close:hover { color: var(--err, #f87171); background: rgba(248, 113, 113, 0.12); }
    .tab-button:focus-visible,
    .tab-close:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: -2px;
    }
    .editors {
      display: flex;
      flex-direction: column;
      flex: 1 1 auto;
      min-width: 0;
      min-height: 0;
    }
    anth-file-editor {
      flex: 1 1 auto;
      min-width: 0;
      min-height: 0;
    }
    anth-file-editor[hidden] { display: none !important; }
    @media (pointer: coarse) {
      .tab-button, .tab-close { min-height: 44px; }
    }
  `
}

export interface OpenFileEditor {
  client: FilesClient
  mode: 'edit' | 'create'
  directory: string
  name: string
}

let editorWindow: AnthWindow | undefined
let editorHost: AnthFileTabs | undefined

/**
 * The Files section has one editor window for the whole page. Opening a file
 * adds a tab to it; the window lives on `document.body`, so navigating the
 * panel cannot close it or lose its tabs.
 */
export function openFileEditor(options: OpenFileEditor): AnthWindow {
  if (!editorWindow?.isConnected || !editorHost) {
    const host = document.createElement('anth-file-tabs') as AnthFileTabs
    host.client = options.client
    const window = document.createElement('anth-window') as AnthWindow
    window.label = msg('Editor')
    window.append(host)
    host.addEventListener(FILE_TABS_EMPTY_EVENT, () => window.close())
    window.addEventListener(WINDOW_CLOSE_EVENT, () => {
      window.remove()
      if (editorWindow === window) {
        editorWindow = undefined
        editorHost = undefined
      }
    })
    document.body.append(window)
    editorWindow = window
    editorHost = host
  }

  const window = editorWindow
  const host = editorHost
  if (!window || !host) {
    throw new Error('the editor window could not be created')
  }
  window.restore()
  window.open = true
  host.open(options)
  host.focusActive()
  return window
}

function uniqueNewLabel(tabs: FileTab[]): string {
  const base = msg('New file')
  const labels = new Set(tabs.map((tab) => tab.label))
  if (!labels.has(base)) {
    return base
  }
  let index = 2
  while (labels.has(`${base} ${index}`)) {
    index += 1
  }
  return `${base} ${index}`
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function byteLength(text: string): number {
  return new TextEncoder().encode(text).length
}
