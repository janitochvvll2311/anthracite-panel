import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { repeat } from 'lit/directives/repeat.js'
import { localized, msg } from '@lit/localize'

import type { FilesClient } from './files-client'
import './file-editor'
import type { AnthFileEditor } from './file-editor'
import { baseName, join } from './paths'
import './icon'

/** Fired when the last tab closes, so the window goes away too. */
export const FILE_TABS_EMPTY_EVENT = 'anth-file-tabs-empty'

interface FileTab {
  key: string
  mode: 'edit' | 'create'
  directory: string
  name: string
  /** Full path on edit tabs; empty on create tabs until a name is typed. */
  path: string
  /** Short label shown in the tab. */
  label: string
  /** Full path shown as the tab tooltip. */
  title: string
}

export interface OpenFileEditor {
  client: FilesClient
  mode: 'edit' | 'create'
  directory: string
  name: string
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
    const path = options.mode === 'edit' ? join(options.directory, options.name) : ''
    if (path) {
      const existing = this.tabs.find((tab) => tab.path === path)
      if (existing) {
        this.activate(existing.key)
        return existing.key
      }
    }
    const key = `tab-${++tabIds}`
    const label = options.mode === 'edit' ? options.name : uniqueNewLabel(this.tabs)
    const title = options.mode === 'edit' ? path : msg('New file')
    this.tabs = [
      ...this.tabs,
      {
        key,
        mode: options.mode,
        directory: options.directory,
        name: options.name,
        path,
        label,
        title,
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
    this.tabs = this.tabs.map((tab) =>
      tab.key === key ? { ...tab, label: baseName(title), title } : tab,
    )
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
                title=${tab.title}
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
                <anth-icon name="close"></anth-icon>
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
      display: inline-grid;
      place-items: center;
      width: 28px;
      height: 28px;
      padding: 0;
      border: 0;
      border-left: 1px solid var(--panel-border, #223041);
      background: transparent;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .tab-close anth-icon { font-size: 12px; }
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
      .tab-close { width: 44px; }
    }
  `
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
