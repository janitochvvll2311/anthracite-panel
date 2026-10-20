import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-volume-manager'
import type { AnthVolumeManager } from './anth-volume-manager'
import { FakeVolumeClient, volumeSnapshot } from '../test/fake-volume-client'
import { answerDialogs, dialogText, resetDialogs } from '../test/dialogs'

async function flush(element: AnthVolumeManager): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(
  client: FakeVolumeClient,
  options: { autoLoad?: boolean } = {},
): Promise<AnthVolumeManager> {
  const element = document.createElement(
    'anth-volume-manager',
  ) as AnthVolumeManager
  element.client = client
  if (options.autoLoad !== undefined) element.autoLoad = options.autoLoad
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

function findButton(
  element: AnthVolumeManager,
  label: string,
): HTMLButtonElement | undefined {
  return [...(element.shadowRoot?.querySelectorAll('button') ?? [])].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
}

function button(element: AnthVolumeManager, label: string): HTMLButtonElement {
  const match = findButton(element, label)
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

function input(element: AnthVolumeManager, selector: string): HTMLInputElement {
  const match = element.shadowRoot?.querySelector<HTMLInputElement>(selector)
  if (!match) throw new Error(`input ${selector} was not rendered`)
  return match
}

function text(element: AnthVolumeManager): string {
  return element.shadowRoot?.textContent ?? ''
}

function setValue(element: HTMLInputElement | HTMLSelectElement, value: string) {
  element.value = value
  element.dispatchEvent(new Event(element.tagName === 'SELECT' ? 'change' : 'input'))
}

afterEach(() => {
  resetDialogs()
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('AnthVolumeManager', () => {
  it('loads the inventory and renders devices and mounts', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client)

    expect(client.get).toHaveBeenCalledTimes(1)
    const body = text(element)
    expect(body).toContain('sda1')
    expect(body).toContain('sdb1')
    expect(body).toContain('ext4')
    expect(body).toContain('/dev/sda1')
    expect(body).toContain('4 devices')
  })

  it('nests a partition under its disk', async () => {
    const element = await mount(new FakeVolumeClient())

    const devices = [
      ...(element.shadowRoot?.querySelectorAll('td.device') ?? []),
    ] as HTMLElement[]
    const depths = devices.map((cell) => cell.style.getPropertyValue('--depth'))
    expect(depths).toEqual(['0', '1', '1', '1'])
  })

  it('does not auto load when disabled', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client, { autoLoad: false })
    expect(client.get).not.toHaveBeenCalled()
    expect(text(element)).toContain('No volume snapshot yet.')
  })

  it('reports a load failure without clearing the layout', async () => {
    const client = new FakeVolumeClient()
    client.getError = new Error('daemon unavailable')
    const element = await mount(client)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'daemon unavailable',
    )
  })

  it('filters devices and mounts together', async () => {
    const element = await mount(new FakeVolumeClient())
    setValue(input(element, 'input[type="search"]'), 'usb')
    await element.updateComplete

    const body = text(element)
    expect(body).toContain('sdb1')
    expect(body).not.toContain('sda1')
    expect(body).toContain('No mounts match the filter.')
  })

  it('keeps the ancestors of a matching child visible', async () => {
    const element = await mount(new FakeVolumeClient())
    // `usb` matches only sdb1; `root` matches only sda1, which has parent sda.
    setValue(input(element, 'input[type="search"]'), 'root')
    await element.updateComplete

    const names = [
      ...(element.shadowRoot?.querySelectorAll('td.device .name') ?? []),
    ].map((node) => node.textContent?.trim())
    expect(names).toEqual(['sda', 'sda1'])
  })

  it('sorts devices by size descending on first click', async () => {
    const element = await mount(new FakeVolumeClient())
    button(element, 'Size').click()
    await element.updateComplete

    const names = [
      ...(element.shadowRoot?.querySelectorAll('td.device .name') ?? []),
    ].map((node) => node.textContent?.trim())
    expect(names).toEqual(['sda', 'sda1', 'sdb1', 'sdc1'])
  })

  it('mounts the selected device with the requested options', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client)

    const select = element.shadowRoot?.querySelector('select')
    if (!select) throw new Error('device select was not rendered')
    setValue(select, '/dev/sdb1')
    await element.updateComplete

    // The removable checkbox only appears for removable media; tick it by
    // locating the label that carries the text.
    const labels = [...(element.shadowRoot?.querySelectorAll('label.check') ?? [])]
    const allow = labels
      .find((label) => label.textContent?.includes('Allow removable'))
      ?.querySelector('input')
    if (allow === null || allow === undefined) {
      throw new Error('allow removable checkbox was not rendered')
    }
    allow.click()
    await element.updateComplete

    setValue(
      input(element, 'input[placeholder="Target directory, e.g. /mnt/data"]'),
      '/mnt/usb',
    )
    await element.updateComplete

    button(element, 'Mount').click()
    await flush(element)

    expect(client.mountCalls).toEqual([
      {
        device: '/dev/sdb1',
        target: '/mnt/usb',
        fs_type: 'vfat',
        read_only: false,
        allow_removable: true,
      },
    ])
    expect(text(element)).toContain('mounted /dev/sdb1 on /mnt/usb')
  })

  it('refuses removable media unless the option is ticked', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client)

    const select = element.shadowRoot?.querySelector('select')
    if (!select) throw new Error('device select was not rendered')
    setValue(select, '/dev/sdb1')
    await element.updateComplete

    setValue(
      input(element, 'input[placeholder="Target directory, e.g. /mnt/data"]'),
      '/mnt/usb',
    )
    await element.updateComplete

    button(element, 'Mount').click()
    await flush(element)

    expect(client.mountCalls).toEqual([])
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'removable',
    )
  })

  it('requires an absolute target', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client)

    setValue(
      input(element, 'input[placeholder="Target directory, e.g. /mnt/data"]'),
      'mnt/data',
    )
    await element.updateComplete

    button(element, 'Mount').click()
    await flush(element)

    expect(client.mountCalls).toEqual([])
    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'absolute path',
    )
  })

  it('surfaces a mount failure from the daemon', async () => {
    const client = new FakeVolumeClient()
    client.mountError = new Error('/mnt/data is outside the allowed mount root')
    const element = await mount(client)

    // The first candidate is removable media, which the form refuses locally.
    const select = element.shadowRoot?.querySelector('select')
    if (!select) throw new Error('device select was not rendered')
    setValue(select, '/dev/sdc1')
    await element.updateComplete

    setValue(
      input(element, 'input[placeholder="Target directory, e.g. /mnt/data"]'),
      '/mnt/data',
    )
    await element.updateComplete
    button(element, 'Mount').click()
    await flush(element)

    expect(element.shadowRoot?.querySelector('.error')?.textContent).toContain(
      'outside the allowed mount root',
    )
  })

  it('unmounts after confirmation and refreshes', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client)
    button(element, 'Unmount').click()
    await flush(element)
    expect(dialogText()).toContain('Unmount /?')
    await answerDialogs('confirm')
    await flush(element)

    expect(client.unmountCalls).toEqual(['/'])
    expect(client.get).toHaveBeenCalledTimes(2)
    expect(text(element)).toContain('unmounted /')
  })

  it('does not unmount when the confirmation is declined', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client)
    button(element, 'Unmount').click()
    await flush(element)
    await answerDialogs('cancel')
    await flush(element)

    expect(client.unmountCalls).toEqual([])
  })

  it('offers no unmount button for a virtual filesystem', async () => {
    const client = new FakeVolumeClient()
    client.current = volumeSnapshot({
      mounts: [
        {
          mountpoint: '/proc',
          source: 'proc',
          fstype: 'proc',
          options: 'rw',
          read_only: false,
          virtual_fs: true,
        },
      ],
    })
    const element = await mount(client)

    expect(findButton(element, 'Unmount')).toBeUndefined()
    expect(text(element)).toContain('virtual')
  })

  it('stays read-only when the daemon disables control', async () => {
    const client = new FakeVolumeClient()
    client.current = volumeSnapshot({ control_enabled: false })
    const element = await mount(client)

    expect(text(element)).toContain('Volume control is disabled')
    expect(findButton(element, 'Mount')).toBeUndefined()
    expect(findButton(element, 'Unmount')).toBeUndefined()
  })

  it('shows the snapshot warning when the list was shortened', async () => {
    const client = new FakeVolumeClient()
    client.current = volumeSnapshot({
      total: 9000,
      truncated: true,
      warning: 'showing 4 of 9000 devices',
    })
    const element = await mount(client)

    expect(element.shadowRoot?.querySelector('.warning')?.textContent).toContain(
      'showing 4 of 9000 devices',
    )
    expect(text(element)).toContain('4 of 9000 devices')
  })

  it('exposes a manual refresh', async () => {
    const client = new FakeVolumeClient()
    const element = await mount(client)

    button(element, 'Refresh').click()
    await flush(element)

    expect(client.get).toHaveBeenCalledTimes(2)
  })
})
