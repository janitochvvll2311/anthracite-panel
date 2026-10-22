import { afterEach, describe, expect, it, vi } from 'vitest'

// The plugin's web module is plain ES module source served from its bundle; it
// is not part of the Vite build. Importing it here exercises the same file the
// browser receives, including the header button wiring that a shadow-root
// listener on the body used to miss.
import '../../../plugins/wireguard/web/wireguard.js'

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('anth-wireguard-panel', () => {
  it('opens the create form from the header action', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify({ interfaces: [] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      ),
    )

    const panel = document.createElement('anth-wireguard-panel')
    document.body.append(panel)
    await settle()

    const button = panel.shadowRoot?.querySelector<HTMLButtonElement>(
      '[data-action="new"]',
    )
    expect(button).not.toBeNull()

    button?.click()
    await settle()

    expect(
      panel.shadowRoot?.querySelector('form[data-form="interface"]'),
    ).not.toBeNull()
  })
})
