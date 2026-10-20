/**
 * Answers the dialog host from a component test.
 *
 * Components ask questions through `lib/ui/dialog`, so `vi.stubGlobal('confirm')`
 * no longer intercepts them. This helper clicks the real buttons instead, which
 * keeps the promise, the queue and the rendered markup under test. Call
 * `resetDialogs` between tests: a question left queued by a previous test would
 * otherwise be the one answered here.
 */

type Answer = 'confirm' | 'cancel'

interface DialogElement extends HTMLElement {
  updateComplete: Promise<boolean>
}

function host(): DialogElement | null {
  return document.querySelector<DialogElement>('anth-dialog-host')
}

function modal(): HTMLElement | null {
  return host()?.shadowRoot?.querySelector('anth-modal') ?? null
}

function button(answer: Answer): HTMLButtonElement | null {
  return (
    host()?.shadowRoot?.querySelector<HTMLButtonElement>(
      `[data-action="${answer}"]`,
    ) ?? null
  )
}

async function settle(): Promise<void> {
  const element = host()
  if (!element) return
  await element.updateComplete
  const frame = modal() as
    | (HTMLElement & { updateComplete?: Promise<unknown> })
    | null
  await frame?.updateComplete
}

/**
 * Answers the dialog that is on screen, if there is one.
 *
 * `value` fills a prompt before confirming. Answers up to four questions so a
 * confirm behind a prompt is covered, and does nothing when no dialog appears,
 * which is the normal case for an action that stops before asking.
 */
export async function answerDialogs(
  answer: Answer = 'confirm',
  value?: string,
): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle()
    if (!modal()) return

    if (value !== undefined) {
      const input = host()?.shadowRoot?.querySelector<HTMLInputElement>('input')
      if (input) {
        input.value = value
        input.dispatchEvent(new Event('input', { bubbles: true }))
        await settle()
      }
    }

    button(answer)?.click()
  }
}

/** Text of the open dialog, including the modal's own heading and buttons. */
export function dialogText(): string {
  const frame = modal()
  if (!frame) return ''
  return (frame.textContent ?? '') + (frame.shadowRoot?.textContent ?? '')
}

/** Removes the host so a queued question cannot outlive its test. */
export function resetDialogs(): void {
  host()?.remove()
}
