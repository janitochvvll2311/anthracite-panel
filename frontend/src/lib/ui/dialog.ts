/**
 * Modal dialogs as promises, replacing `window.confirm`, `prompt` and `alert`.
 *
 * The native functions block the event loop, cannot be styled or translated in
 * place, and on a page that also runs a WebSocket terminal they freeze the
 * session until the operator clicks. These helpers return a promise and route
 * through one `anth-dialog-host` element, so a call site reads like the one it
 * replaced:
 *
 * ```ts
 * if (!(await confirmAction({ title: 'Stop the daemon?', danger: true }))) return
 * ```
 *
 * One dialog is visible at a time; a second request queues behind the first,
 * because two stacked confirmations from one click is a bug in the caller, not
 * something the operator can answer.
 */

import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, state } from 'lit/decorators.js'

import '../../components/anth-modal'

export interface ConfirmOptions {
  title: string
  /** Body text. Omitted when the title is the whole question. */
  message?: string
  confirmLabel?: string
  cancelLabel?: string
  /** Marks the confirm button as destructive. */
  danger?: boolean
}

export interface PromptOptions extends ConfirmOptions {
  /** Visible label for the field. */
  label: string
  value?: string
  placeholder?: string
  /** Returns a message when the value must not be submitted. */
  validate?: (value: string) => string | undefined
}

type Kind = 'confirm' | 'prompt' | 'alert'

/** One dialog's data plus the way to answer it. */
interface Dialog {
  kind: Kind
  options: ConfirmOptions & Partial<PromptOptions>
  /** Resolves the caller's promise and clears the dialog. */
  settle: (accepted: boolean, value: string) => void
}

@customElement('anth-dialog-host')
export class AnthDialogHost extends LitElement {
  /** The dialog on screen, if any. */
  @state()
  private current?: Dialog

  @state()
  private value = ''

  @state()
  private error = ''

  /** Requests waiting for their turn, oldest first. */
  private queue: Array<() => void> = []

  /**
   * Shows one dialog and resolves with its answer.
   *
   * `initial` is the text a prompt starts with; the caller's resolver receives
   * `false`, `true` or the text, and the shape is fixed by the helper below.
   */
  ask<T>(
    kind: Kind,
    options: ConfirmOptions & Partial<PromptOptions>,
    initial: string,
    answer: (accepted: boolean, value: string) => T,
  ): Promise<T> {
    const host = hostElement()
    return new Promise<T>((resolve) => {
      const start = (): void => {
        host.current = {
          kind,
          options,
          settle: (accepted, value) => {
            host.current = undefined
            host.value = ''
            host.error = ''
            const next = host.queue.shift()
            if (next) next()
            resolve(answer(accepted, value))
          },
        }
        host.value = initial
        host.error = ''
      }

      if (host.current) host.queue.push(start)
      else start()
    })
  }

  private finish(accepted: boolean): void {
    const dialog = this.current
    if (!dialog) return

    if (accepted && dialog.kind === 'prompt') {
      const problem = dialog.options.validate?.(this.value)
      if (problem) {
        // A rejected value keeps the dialog open: closing it would throw away
        // what the operator typed.
        this.error = problem
        return
      }
    }

    dialog.settle(accepted, this.value)
  }

  private onInput(event: Event): void {
    this.value = (event.target as HTMLInputElement).value
    this.error = ''
  }

  private onModalClose(event: Event): void {
    event.stopPropagation()
    // Escape, the close button and the backdrop all mean "no".
    this.finish(false)
  }

  private onSubmit(event: Event): void {
    event.preventDefault()
    this.finish(true)
  }

  override render() {
    const dialog = this.current
    if (!dialog) return nothing

    const { options, kind } = dialog
    const prompt = kind === 'prompt'
    const confirmLabel =
      options.confirmLabel ?? (kind === 'alert' ? 'OK' : 'Confirm')

    return html`
      <anth-modal
        open
        size="sm"
        heading=${options.title}
        @anth-modal-close=${this.onModalClose}
      >
        <form @submit=${this.onSubmit} data-kind=${kind}>
          ${options.message
            ? html`<p class="message">${options.message}</p>`
            : nothing}
          ${prompt
            ? html`
                <label class="field">
                  <span>${options.label}</span>
                  <input
                    autofocus
                    .value=${this.value}
                    placeholder=${options.placeholder ?? ''}
                    aria-invalid=${this.error !== ''}
                    aria-describedby=${this.error ? 'dialog-error' : nothing}
                    @input=${this.onInput}
                  />
                </label>
                ${this.error
                  ? html`<p class="error" id="dialog-error" role="alert">
                      ${this.error}
                    </p>`
                  : nothing}
              `
            : nothing}

          <div slot="footer">
            ${kind === 'alert'
              ? nothing
              : html`<button
                  type="button"
                  data-action="cancel"
                  @click=${() => this.finish(false)}
                >
                  ${options.cancelLabel ?? 'Cancel'}
                </button>`}
            <button
              type="submit"
              class=${options.danger ? 'danger' : 'primary'}
              data-action="confirm"
            >
              ${confirmLabel}
            </button>
          </div>
        </form>
      </anth-modal>
    `
  }

  protected override updated(changed: PropertyValues): void {
    // A prompt opens with its text selected, so typing replaces the old name
    // instead of appending to it.
    if (!changed.has('current') || this.current?.kind !== 'prompt') return
    const input = this.renderRoot.querySelector('input')
    input?.focus()
    input?.select()
  }

  static styles = css`
    :host {
      display: contents;
    }

    .message {
      margin: 0 0 12px;
      color: var(--muted, #8b9bad);
      white-space: pre-wrap;
    }

    .field {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    .field span {
      color: var(--muted, #8b9bad);
      font-size: 12px;
    }

    input {
      padding: 6px 9px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      font: inherit;
    }

    input:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 1px;
    }

    .error {
      margin: 8px 0 0;
      color: #fca5a5;
      font-size: 12px;
    }

    button {
      padding: 5px 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      font: inherit;
      cursor: pointer;
    }

    button:hover {
      border-color: var(--accent, #7dd3fc);
    }

    button:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .primary {
      border-color: var(--accent, #7dd3fc);
      color: var(--accent, #7dd3fc);
    }

    .danger {
      border-color: #fca5a5;
      color: #fca5a5;
    }
  `
}

/**
 * The document's dialog host, created on first use.
 *
 * One host keeps the queue and the focus handling in a single place; a
 * component that asked a question is never the element that owns the dialog,
 * so unmounting it mid-question cannot strand the promise.
 */
function hostElement(): AnthDialogHost {
  const existing = document.querySelector<AnthDialogHost>('anth-dialog-host')
  if (existing) return existing
  const host = document.createElement('anth-dialog-host') as AnthDialogHost
  document.body.append(host)
  return host
}

/** Asks a yes/no question. Escape, the close button and the backdrop mean no. */
export function confirmAction(options: ConfirmOptions): Promise<boolean> {
  return hostElement().ask('confirm', options, '', (accepted) => accepted)
}

/** Asks for one line of text. Resolves `undefined` when cancelled. */
export function promptText(options: PromptOptions): Promise<string | undefined> {
  return hostElement().ask(
    'prompt',
    options,
    options.value ?? '',
    (accepted, value) => (accepted ? value : undefined),
  )
}

/** Reports something the operator has to acknowledge. */
export async function showAlert(options: ConfirmOptions): Promise<void> {
  await hostElement().ask('alert', options, '', () => undefined)
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-dialog-host': AnthDialogHost
  }
}
