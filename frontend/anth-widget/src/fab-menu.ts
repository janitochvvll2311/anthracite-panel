import { escape } from './html'
import { FAB_STYLE } from './fab'

/**
 * `anth-fab-menu`: a floating action button that expands a small menu.
 *
 * The consumer declares the menu entries in the light DOM:
 *
 * ```html
 * <anth-fab-menu icon="plus" label="File actions">
 *   <button data-action="create">New file</button>
 *   <button data-action="upload">Upload</button>
 * </anth-fab-menu>
 * ```
 *
 * The menu closes itself after an entry is chosen, on Escape or on an outside
 * pointer. The FAB itself is icon-only, so it keeps working on narrow screens.
 */
export class AnthFabMenu extends HTMLElement {
  static get observedAttributes(): string[] {
    return ['icon', 'label', 'disabled', 'open']
  }

  get icon(): string {
    return this.getAttribute('icon') ?? 'plus'
  }

  set icon(value: string) {
    this.setAttribute('icon', value)
  }

  get label(): string {
    return this.getAttribute('label') ?? ''
  }

  set label(value: string) {
    this.setAttribute('label', value)
  }

  get open(): boolean {
    return this.hasAttribute('open')
  }

  set open(value: boolean) {
    this.toggleAttribute('open', value)
  }

  get disabled(): boolean {
    return this.hasAttribute('disabled')
  }

  set disabled(value: boolean) {
    this.toggleAttribute('disabled', value)
  }

  #mounted = false

  #wired = false

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: 'open' })
    }
    if (!this.#mounted) {
      this.#mounted = true
      this.#paint()
    }
    if (!this.#wired) {
      this.#wired = true
      this.addEventListener('click', this.#onHostClick)
    }
    this.#sync()
  }

  disconnectedCallback(): void {
    if (this.#wired) {
      this.#wired = false
      this.removeEventListener('click', this.#onHostClick)
    }
    document.removeEventListener('pointerdown', this.#onOutside, true)
    document.removeEventListener('keydown', this.#onKeydown)
  }

  attributeChangedCallback(name: string): void {
    if (name === 'open') {
      this.#sync()
    } else {
      this.#paint()
    }
  }

  #paint(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    root.innerHTML = `
      <style>${FAB_STYLE}${MENU_STYLE}</style>
      <div class="menu" role="menu" hidden><slot></slot></div>
      <button type="button" class="fab" aria-haspopup="menu" aria-expanded="false"
              aria-label="${escape(this.label)}" title="${escape(this.label)}"${this.disabled ? ' disabled' : ''}>
        <anth-icon name="${escape(this.icon)}"></anth-icon>
      </button>
    `
    root.querySelector('.fab')?.addEventListener('click', () => {
      if (!this.disabled) {
        this.open = !this.open
      }
    })
    this.#sync()
  }

  #sync(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    const menu = root.querySelector<HTMLElement>('.menu')
    const button = root.querySelector<HTMLButtonElement>('.fab')
    if (menu) {
      menu.hidden = !this.open
    }
    button?.setAttribute('aria-expanded', String(this.open))
    this.toggleAttribute('data-open', this.open)
    if (this.open) {
      document.addEventListener('pointerdown', this.#onOutside, true)
      document.addEventListener('keydown', this.#onKeydown)
    } else {
      document.removeEventListener('pointerdown', this.#onOutside, true)
      document.removeEventListener('keydown', this.#onKeydown)
    }
  }

  /**
   * Any button, link or label in the menu chooses an action.
   *
   * A file input keeps the menu open: touching it starts the native picker,
   * and hiding its ancestor before the picker appears cancels the selection
   * on mobile browsers. The owner closes the menu on `change` or `cancel`.
   */
  readonly #onHostClick = (event: MouseEvent): void => {
    if (!this.open) {
      return
    }
    const path = event.composedPath()
    if (
      path.some(
        (node) => node instanceof HTMLInputElement && node.type === 'file',
      )
    ) {
      return
    }
    const chose = path.some(
      (node) =>
        node instanceof HTMLButtonElement ||
        node instanceof HTMLAnchorElement ||
        node instanceof HTMLLabelElement,
    )
    if (chose && !path.includes(this.shadowRoot?.querySelector('.fab') ?? this)) {
      this.open = false
    }
  }

  readonly #onOutside = (event: Event): void => {
    if (!event.composedPath().includes(this)) {
      this.open = false
    }
  }

  readonly #onKeydown = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      this.open = false
    }
  }
}

const MENU_STYLE = `
  .menu {
    position: absolute;
    right: 0;
    bottom: 64px;
    display: grid;
    gap: 6px;
    min-width: 12rem;
    padding: 6px;
    border: 1px solid var(--panel-border, #223041);
    border-radius: 10px;
    background: var(--panel, #101721);
    box-shadow: 0 14px 32px rgba(0, 0, 0, 0.45);
  }
  .menu[hidden] { display: none; }
  button > anth-icon { transition: transform 150ms ease; }
  :host([data-open]) button > anth-icon { transform: rotate(45deg); }
  ::slotted(button), ::slotted(a), ::slotted(label) {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 42px;
    padding: 8px 10px;
    border: 1px solid var(--panel-border, #223041);
    border-radius: 7px;
    background: rgba(125, 211, 252, 0.06);
    color: var(--text, #dbe6f2);
    font: 12px/1.4 var(--mono, monospace);
    text-align: left;
    cursor: pointer;
  }
  ::slotted(button:hover), ::slotted(a:hover), ::slotted(label:hover) {
    background: rgba(125, 211, 252, 0.14);
  }
  ::slotted(button:focus-visible), ::slotted(a:focus-visible), ::slotted(label:focus-visible) {
    outline: 2px solid var(--accent, #7dd3fc);
    outline-offset: -2px;
  }
  @media (prefers-reduced-motion: reduce) {
    button > anth-icon, :host([data-open]) button > anth-icon { transition: none; }
  }
  @media (max-width: 48rem) {
    .menu { min-width: 0; }
    ::slotted(button), ::slotted(a), ::slotted(label) {
      justify-content: center;
      min-width: 44px;
      min-height: 44px;
      padding: 0;
    }
  }
`

if (!customElements.get('anth-fab-menu')) {
  customElements.define('anth-fab-menu', AnthFabMenu)
}
