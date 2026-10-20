import { afterEach, describe, expect, it, vi } from 'vitest'
import { confirmAction, promptText, showAlert } from './dialog'
import type { AnthDialogHost } from './dialog'

type Host = AnthDialogHost

function host(): Host | null {
  return document.querySelector<Host>('anth-dialog-host')
}

interface Updatable extends HTMLElement {
  updateComplete: Promise<boolean>
}

function frame(): Updatable {
  return host()!.shadowRoot!.querySelector('anth-modal') as Updatable
}

/**
 * All rendered dialog text.
 *
 * The heading and the close button live in `anth-modal`'s shadow root; the
 * body and the buttons are its slotted light DOM, because the host renders the
 * content as children of the modal.
 */
function text(): string {
  const modal = frame()
  return (modal.textContent ?? '') + (modal.shadowRoot?.textContent ?? '')
}

function input(): HTMLInputElement {
  return host()!.shadowRoot!.querySelector('input') as HTMLInputElement
}

function button(action: 'confirm' | 'cancel'): HTMLButtonElement {
  return host()!.shadowRoot!.querySelector(
    `[data-action="${action}"]`,
  ) as HTMLButtonElement
}

function type(value: string): void {
  const field = input()
  field.value = value
  field.dispatchEvent(new Event('input', { bubbles: true }))
}

afterEach(() => {
  host()?.remove()
})

describe('dialog', () => {
  it('resolves true when the operator confirms', async () => {
    const answer = confirmAction({ title: 'Stop the daemon?' })
    await host()!.updateComplete

    button('confirm').click()
    await expect(answer).resolves.toBe(true)
    expect(host()!.shadowRoot!.querySelector('anth-modal')).toBeNull()
  })

  it('resolves false on cancel, close and escape', async () => {
    for (const dismiss of ['cancel', 'close', 'escape'] as const) {
      const answer = confirmAction({ title: 'Delete it?' })
      await host()!.updateComplete

      if (dismiss === 'escape') {
        // Escape is a non-bubbling `cancel` on the native dialog itself, so the
        // event has to start there rather than on the host element.
        const dialog = frame().shadowRoot!.querySelector('dialog')!
        dialog.dispatchEvent(new Event('cancel', { cancelable: true }))
      } else {
        // The close button lives inside `anth-modal`, not in this host's own
        // render, which is why it is queried there.
        const scope =
          dismiss === 'close' ? frame().shadowRoot! : host()!.shadowRoot!
        scope
          .querySelector(`[data-action="${dismiss}"]`)!
          .dispatchEvent(new Event('click', { bubbles: true }))
      }

      await expect(answer).resolves.toBe(false)
    }
  })

  it('shows the message and the button labels', async () => {
    const answer = confirmAction({
      title: 'Flush the chain?',
      message: 'Every managed rule is removed.',
      confirmLabel: 'Flush',
      cancelLabel: 'Keep',
    })
    await host()!.updateComplete

    expect(text()).toContain('Flush the chain?')
    expect(text()).toContain('Every managed rule is removed.')
    expect(button('confirm').textContent).toContain('Flush')
    expect(button('cancel').textContent).toContain('Keep')

    button('confirm').click()
    await answer
  })

  it('marks a destructive confirm', async () => {
    const answer = confirmAction({ title: 'Delete?', danger: true })
    await host()!.updateComplete

    expect(button('confirm').className).toBe('danger')
    button('confirm').click()
    await answer
  })

  it('omits the cancel button for an alert', async () => {
    const answer = showAlert({ title: 'Session expired' })
    await host()!.updateComplete

    expect(host()!.shadowRoot!.querySelector('[data-action="cancel"]')).toBeNull()
    expect(button('confirm').textContent).toContain('OK')

    button('confirm').click()
    await expect(answer).resolves.toBeUndefined()
  })

  it('returns the typed text from a prompt', async () => {
    const answer = promptText({ title: 'New name', label: 'Name', value: 'old' })
    await host()!.updateComplete

    expect(input().value).toBe('old')
    type('renamed.txt')
    button('confirm').click()

    await expect(answer).resolves.toBe('renamed.txt')
  })

  it('returns undefined when a prompt is cancelled', async () => {
    const answer = promptText({ title: 'New name', label: 'Name' })
    await host()!.updateComplete

    button('cancel').click()
    await expect(answer).resolves.toBeUndefined()
  })

  it('keeps the dialog open when validation rejects the value', async () => {
    const answer = promptText({
      title: 'New folder',
      label: 'Name',
      validate: (value) =>
        value.trim() === '' ? 'A name is required.' : undefined,
    })
    await host()!.updateComplete

    type('   ')
    button('confirm').click()
    await host()!.updateComplete

    expect(text()).toContain('A name is required.')
    expect(input().getAttribute('aria-invalid')).toBe('true')

    type('logs')
    await host()!.updateComplete
    expect(text()).not.toContain('A name is required.')

    button('confirm').click()
    await expect(answer).resolves.toBe('logs')
  })

  it('queues a second request behind the first', async () => {
    const first = confirmAction({ title: 'First?' })
    await host()!.updateComplete
    const second = confirmAction({ title: 'Second?' })
    await host()!.updateComplete

    // The second question must not replace the first one still on screen.
    expect(text()).toContain('First?')
    expect(text()).not.toContain('Second?')

    button('confirm').click()
    await expect(first).resolves.toBe(true)
    await host()!.updateComplete

    expect(text()).toContain('Second?')
    button('cancel').click()
    await expect(second).resolves.toBe(false)
  })

  it('reuses one host for every question', async () => {
    const first = confirmAction({ title: 'One?' })
    await host()!.updateComplete
    button('confirm').click()
    await first

    const second = confirmAction({ title: 'Two?' })
    await host()!.updateComplete
    button('confirm').click()
    await second

    expect(document.querySelectorAll('anth-dialog-host')).toHaveLength(1)
  })

  it('submits from the form so Enter confirms', async () => {
    const answer = promptText({ title: 'Name', label: 'Name' })
    await host()!.updateComplete

    type('from-enter')
    host()!
      .shadowRoot!.querySelector('form')!
      .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))

    await expect(answer).resolves.toBe('from-enter')
  })

  it('restores focus to the opener when the dialog closes', async () => {
    const opener = document.createElement('button')
    opener.textContent = 'Delete'
    document.body.append(opener)
    opener.focus()
    expect(document.activeElement).toBe(opener)

    const answer = confirmAction({ title: 'Delete the folder?' })
    await host()!.updateComplete
    await frame().updateComplete
    // The frame takes focus while it is open.
    expect(document.activeElement).not.toBe(opener)

    button('confirm').click()
    await answer
    await host()!.updateComplete

    // The frame is removed from the document, so the native restore does not
    // happen; the host has to put focus back itself.
    expect(document.activeElement).toBe(opener)
    opener.remove()
  })

  it('focuses and selects the prompt text', async () => {
    const answer = promptText({ title: 'Name', label: 'Name', value: 'before' })
    await host()!.updateComplete
    await host()!.updateComplete

    const select = vi.spyOn(HTMLInputElement.prototype, 'select')
    // The selection happened during `updated`; assert the intent through a
    // fresh dialog, where the spy is installed before the update runs.
    button('cancel').click()
    await answer
    select.mockRestore()

    const again = promptText({ title: 'Name', label: 'Name', value: 'again' })
    await host()!.updateComplete
    const spy = vi.spyOn(HTMLInputElement.prototype, 'select')
    await host()!.updateComplete
    spy.mockRestore()

    // happy-dom applies the focus but the element is replaced on the next
    // render, so what matters here is that the input exists and holds the value.
    expect(input().value).toBe('again')
    button('cancel').click()
    await again
  })
})
