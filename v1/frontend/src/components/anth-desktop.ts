import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'

import { msg } from '@lit/localize'

import './anth-window'
import {
  ANTH_WINDOW_POPOUT_EVENT,
  type WindowIdDetail,
  type WindowMoveDetail,
} from './anth-window'
import {
  clampRect,
  fitToViewport,
  initialRect,
  loadWindows,
  nextZ,
  saveWindows,
  type Viewport,
  type WindowState,
} from '../lib/ui/windows'

/**
 * The floating-window area: one slot per open panel, a taskbar, and the layout.
 *
 * The desktop owns the state because two features need it. The layout has to
 * survive a reload (`localStorage`, so a pinned arrangement is not undone by a
 * refresh), and the z order has to be global, which a per-window counter cannot
 * express. A window is a thin view over one entry here.
 *
 * A minimized window is *detached*, not hidden. Hiding it with CSS keeps its
 * SSE stream and its terminal WebSocket open, and that is exactly the cost this
 * mode exists to avoid. A detached custom element keeps its properties and its
 * shadow root, so restoring it resumes where it stopped.
 */

export const ANTH_DESKTOP_OPEN_EVENT = 'anth-desktop-open'

export interface DesktopPanel {
  id: string
  heading: string
}

export interface DesktopOpenDetail {
  id: string
  minimize?: boolean
}

@customElement('anth-desktop')
export class AnthDesktop extends LitElement {
  /** Panels this desktop can show. The slotted element for each is `<id>`. */
  @property({ attribute: false })
  panels: DesktopPanel[] = []

  /** Layout storage key, so two desktops do not share a geometry. */
  @property({ attribute: 'storage-key' })
  storageKey = 'main'

  /** Id of the panel to open on first render. Omitted, none is open. */
  @property({ attribute: 'initial-panel' })
  initialPanel = ''

  /** Hides the pop-out button, for a harness that has no window to open. */
  @property({ type: Boolean, attribute: 'no-popout' })
  noPopout = false

  /**
   * Builds the element for a panel. Called once per opened window; the desktop
   * keeps the result, so a minimize and a restore reuse the same instance.
   */
  @property({ attribute: false })
  createPanel?: (id: string) => HTMLElement

  @state()
  private windows: WindowState[] = []

  @state()
  private activeId = ''

  /** Detached bodies, kept across minimize/restore. */
  private readonly parked = new Map<string, HTMLElement>()

  private viewport: Viewport = { width: 1024, height: 768 }

  private readonly onResize = (): void => {
    this.viewport = this.readViewport()
    this.windows = this.windows.map((window) => ({
      ...window,
      rect: fitToViewport(window.rect, this.viewport),
    }))
  }

  override connectedCallback(): void {
    super.connectedCallback()
    this.viewport = this.readViewport()
    globalThis.addEventListener?.('resize', this.onResize)
    this.restore()
  }

  override disconnectedCallback(): void {
    globalThis.removeEventListener?.('resize', this.onResize)
    super.disconnectedCallback()
  }

  /** Reads a stored layout and brings it into the current viewport. */
  private restore(): void {
    const stored = loadWindows(this.storageKey)
    const known = new Set(this.panels.map((panel) => panel.id))
    this.windows = stored
      .filter((window) => known.has(window.id))
      .map((window) => ({
        ...window,
        rect: fitToViewport(window.rect, this.viewport),
      }))
    this.activeId = this.topWindow(this.windows.filter((w) => !w.minimized))?.id ?? ''
  }

  /**
   * Opens a panel, or brings it forward if it is already open.
   *
   * A second request for an open panel must not build a second copy: it would
   * start a second polling stream against the same daemon.
   */
  open(id: string, options: { minimize?: boolean } = {}): void {
    if (!this.panels.some((panel) => panel.id === id)) return
    const existing = this.windows.find((window) => window.id === id)
    if (existing) {
      this.mutate(id, (window) => ({
        ...window,
        minimized: options.minimize === true ? true : false,
        z: nextZ(this.windows),
      }))
      if (!options.minimize) this.activeId = id
      return
    }

    const rect = initialRect(this.windows.length, this.viewport)
    this.windows = [
      ...this.windows,
      {
        id,
        rect,
        minimized: options.minimize === true,
        z: nextZ(this.windows),
      },
    ]
    if (!options.minimize) this.activeId = id
    this.persist()
  }

  close(id: string): void {
    this.parked.get(id)?.remove()
    this.parked.delete(id)
    this.windows = this.windows.filter((window) => window.id !== id)
    if (this.activeId === id) {
      this.activeId = this.topWindow(this.windows.filter((w) => !w.minimized))?.id ?? ''
    }
    this.persist()
  }

  minimize(id: string): void {
    this.mutate(id, (window) => ({ ...window, minimized: true }))
    if (this.activeId === id) {
      const next = this.topWindow(this.windows.filter((w) => !w.minimized && w.id !== id))
      this.activeId = next?.id ?? ''
    }
  }

  restoreWindow(id: string): void {
    this.mutate(id, (window) => ({
      ...window,
      minimized: false,
      z: nextZ(this.windows),
    }))
    this.activeId = id
  }

  raise(id: string): void {
    if (this.activeId === id) return
    this.mutate(id, (window) => ({ ...window, z: nextZ(this.windows) }))
    this.activeId = id
  }

  private mutate(id: string, change: (window: WindowState) => WindowState): void {
    this.windows = this.windows.map((window) =>
      window.id === id ? change(window) : window,
    )
    this.persist()
  }

  private topWindow(windows: readonly WindowState[]): WindowState | undefined {
    return windows.reduce<WindowState | undefined>(
      (top, window) => (top === undefined || window.z > top.z ? window : top),
      undefined,
    )
  }

  private persist(): void {
    saveWindows(this.storageKey, this.windows)
  }

  private readViewport(): Viewport {
    return {
      width: globalThis.innerWidth || 1024,
      height: globalThis.innerHeight || 768,
    }
  }

  /**
   * Opens the requested panel whenever it changes.
   *
   * The shell keeps one desktop and reuses it across panel routes, so this is
   * how navigating to another panel opens a second window instead of replacing
   * the first one.
   */
  protected override updated(changed: PropertyValues<this>): void {
    this.syncBodies()
    if (!changed.has('initialPanel') || !this.initialPanel) return
    this.open(this.initialPanel)
  }

  /**
   * Moves each panel element between its slot and the parked map.
   *
   * A visible window gets its element mounted; a minimized one has it detached
   * but alive. `remove()` only disconnects, so the element keeps its shadow
   * root, its properties and its timers' identity, and `append()` puts it back
   * without re-running the constructor.
   */
  private syncBodies(): void {
    for (const window of this.windows) {
      const slot = this.renderRoot.querySelector<HTMLElement>(
        `.slot[data-slot="${window.id}"]`,
      )
      const body = this.body(window.id)
      if (!body) continue

      if (window.minimized) {
        if (body.isConnected) body.remove()
        continue
      }
      if (slot && body.parentElement !== slot) slot.append(body)
    }
  }

  private body(id: string): HTMLElement | undefined {
    const existing = this.parked.get(id)
    if (existing) return existing
    if (!this.createPanel) return undefined
    const created = this.createPanel(id)
    this.parked.set(id, created)
    return created
  }

  override render() {
    const open = this.windows.map((window) => ({
      window,
      panel: this.panels.find((panel) => panel.id === window.id),
    }))

    return html`
      <div class="desktop">
        ${open.map(({ window, panel }) =>
          panel
            ? html`<anth-window
                window-id=${window.id}
                heading=${panel.heading}
                .rect=${clampRect(window.rect, this.viewport)}
                ?minimized=${window.minimized}
                .z=${window.z}
                ?active=${this.activeId === window.id}
                ?can-popout=${!this.noPopout}
                @anth-window-move=${this.onWindowMove}
                @anth-window-resize=${this.onWindowMove}
                @anth-window-focus=${this.onWindowFocus}
                @anth-window-minimize=${this.onWindowMinimize}
                @anth-window-popout=${this.onWindowPopout}
                @anth-window-close=${this.onWindowClose}
              >
                <div class="slot" data-slot=${window.id}></div>
              </anth-window>`
            : nothing,
        )}
      </div>
      <div class="taskbar" role="toolbar" aria-label=${msg('Open windows')}>
        ${this.windows.length === 0
          ? html`<span class="empty">${msg('No open windows.')}</span>`
          : this.windows.map(
              (window) => html`<button
                type="button"
                class="task"
                ?data-active=${this.activeId === window.id}
                ?data-minimized=${window.minimized}
                aria-pressed=${this.activeId === window.id}
                @click=${() =>
                  window.minimized
                    ? this.restoreWindow(window.id)
                    : this.raise(window.id)}
              >
                ${this.panels.find((panel) => panel.id === window.id)?.heading ??
                window.id}
              </button>`,
            )}
      </div>
    `
  }

  private readonly onWindowMove = (event: CustomEvent<WindowMoveDetail>): void => {
    event.stopPropagation()
    this.mutate(event.detail.id, (window) => ({
      ...window,
      rect: event.detail.rect,
    }))
  }

  private readonly onWindowFocus = (event: CustomEvent<WindowIdDetail>): void => {
    event.stopPropagation()
    this.raise(event.detail.id)
  }

  private readonly onWindowClose = (event: CustomEvent<WindowIdDetail>): void => {
    event.stopPropagation()
    this.close(event.detail.id)
  }

  private readonly onWindowMinimize = (event: CustomEvent<WindowIdDetail>): void => {
    event.stopPropagation()
    this.minimize(event.detail.id)
  }

  private readonly onWindowPopout = (event: CustomEvent<WindowIdDetail>): void => {
    event.stopPropagation()
    this.dispatchEvent(
      new CustomEvent<WindowIdDetail>(ANTH_WINDOW_POPOUT_EVENT, {
        detail: { id: event.detail.id },
        bubbles: true,
        composed: true,
      }),
    )
  }

  static override styles = css`
    :host {
      display: block;
      position: relative;
      min-block-size: 24rem;
    }

    .desktop {
      position: absolute;
      inset: 0;
      overflow: hidden;
      border-radius: 12px;
      background:
        radial-gradient(
          circle at 20% 15%,
          light-dark(#e8ecf6, #232a33),
          transparent 55%
        ),
        light-dark(#f7f8fb, #171b20);
    }

    .slot {
      block-size: 100%;
      overflow: auto;
    }

    .taskbar {
      display: flex;
      flex-wrap: wrap;
      gap: 0.35rem;
      align-items: center;
      padding: 0.4rem 0.5rem;
      border-radius: 0 0 12px 12px;
      background: light-dark(#eef1f6, #1e242b);
    }

    .empty {
      color: light-dark(#6b7280, #9aa4b1);
      font-size: 0.8rem;
    }

    .task {
      border: 1px solid transparent;
      border-radius: 6px;
      padding: 0.25rem 0.6rem;
      background: none;
      color: inherit;
      cursor: pointer;
      font: inherit;
      font-size: 0.8rem;
    }

    .task:hover {
      background: light-dark(#e2e6ec, #2f363d);
    }

    .task[data-active] {
      border-color: light-dark(#6b7fd7, #8e9ff0);
      background: light-dark(#ffffff, #262d35);
    }

    .task[data-minimized] {
      opacity: 0.65;
      font-style: italic;
    }
  `
}
