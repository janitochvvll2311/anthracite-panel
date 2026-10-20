import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-account-manager'
import type { AnthAccountManager } from './anth-account-manager'
import {
  FakeAccountClient,
  accountGroup,
  accountSnapshot,
  accountUser,
} from '../test/fake-account-client'
import { answerDialogs, resetDialogs } from '../test/dialogs'

async function flush(element: AnthAccountManager): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

async function mount(client: FakeAccountClient): Promise<AnthAccountManager> {
  const element = document.createElement('anth-account-manager') as AnthAccountManager
  element.client = client
  document.body.append(element)
  await element.updateComplete
  await flush(element)
  return element
}

function buttons(element: AnthAccountManager): HTMLButtonElement[] {
  return [...(element.shadowRoot?.querySelectorAll('button') ?? [])]
}

function button(element: AnthAccountManager, label: string): HTMLButtonElement {
  const match = buttons(element).find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!match) throw new Error(`button ${label} was not rendered`)
  return match
}

function maybeButton(element: AnthAccountManager, label: string) {
  return buttons(element).find((candidate) => candidate.textContent?.trim() === label)
}

/** Buttons inside a row, found by the row's first cell text. */
function rowButton(element: AnthAccountManager, row: string, label: string) {
  const rows = [...(element.shadowRoot?.querySelectorAll('tbody tr') ?? [])]
  const match = rows.find((candidate) =>
    candidate.querySelector('td')?.textContent?.includes(row),
  )
  if (!match) throw new Error(`row ${row} was not rendered`)
  return [...match.querySelectorAll('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
}

function text(element: AnthAccountManager): string {
  return element.shadowRoot?.textContent ?? ''
}

function input(element: AnthAccountManager, selector: string): HTMLInputElement {
  const match = element.shadowRoot?.querySelector<HTMLInputElement>(selector)
  if (!match) throw new Error(`input ${selector} was not rendered`)
  return match
}

function setValue(element: HTMLInputElement, value: string): void {
  element.value = value
  element.dispatchEvent(new Event('input'))
}

afterEach(() => {
  resetDialogs()
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

describe('loading', () => {
  it('renders the users table with a state per row', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({
        users: [
          accountUser({ name: 'alice', uid: 1001, locked: false }),
          accountUser({ name: 'bob', uid: 1002, locked: true }),
          accountUser({ name: 'carol', uid: 1003, locked: null }),
        ],
      }),
    )
    const element = await mount(client)

    expect(client.get).toHaveBeenCalledTimes(1)
    expect(text(element)).toContain('alice')
    expect(text(element)).toContain('locked')
    expect(text(element)).toContain('unlocked')
    // Unknown must be visible as such, not silently reported as unlocked.
    expect(text(element)).toContain('unknown')
  })

  it('shows the daemon error instead of an empty table', async () => {
    const client = new FakeAccountClient()
    client.getError = new Error('account control is disabled')
    const element = await mount(client)

    expect(text(element)).toContain('account control is disabled')
  })

  it('shows the warning when the snapshot was shortened', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({ total_users: 9000, warning: 'users shortened' }),
    )
    const element = await mount(client)
    expect(text(element)).toContain('users shortened')
  })
})

describe('read-only mode', () => {
  it('hides every mutation when control is off', async () => {
    const client = new FakeAccountClient(accountSnapshot({ control_enabled: false }))
    const element = await mount(client)

    expect(maybeButton(element, 'New user')).toBeUndefined()
    expect(rowButton(element, 'alice', 'Edit')).toBeUndefined()
    expect(text(element)).toContain('read-only')
  })

  it('hides the password action when only passwords are disabled', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({ control_enabled: true, password_enabled: false }),
    )
    const element = await mount(client)

    expect(rowButton(element, 'alice', 'Edit')).toBeDefined()
    expect(rowButton(element, 'alice', 'Password')).toBeUndefined()
  })

  it('shows the password action when both switches are on', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({ control_enabled: true, password_enabled: true }),
    )
    const element = await mount(client)
    expect(rowButton(element, 'alice', 'Password')).toBeDefined()
  })
})

describe('guards mirrored in the UI', () => {
  it('refuses to edit a directory-service account', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({ users: [accountUser({ name: 'domain', source: 'nss' })] }),
    )
    const element = await mount(client)

    expect(rowButton(element, 'domain', 'Edit')).toBeUndefined()
    expect(text(element)).toContain('read-only')
  })

  it('refuses to edit uid 0 even when system accounts are allowed', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({
        allow_system: true,
        users: [accountUser({ name: 'root', uid: 0, system: true })],
      }),
    )
    const element = await mount(client)

    expect(rowButton(element, 'root', 'Edit')).toBeUndefined()
    expect(text(element)).toContain('uid 0')
  })

  it('refuses a system account without allow_system', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({ users: [accountUser({ name: 'daemon', system: true })] }),
    )
    const element = await mount(client)
    expect(rowButton(element, 'daemon', 'Edit')).toBeUndefined()
    expect(text(element)).toContain('allow_system')
  })

  it('allows a system account once allow_system is on', async () => {
    const client = new FakeAccountClient(
      accountSnapshot({
        allow_system: true,
        users: [accountUser({ name: 'daemon', uid: 105, system: true })],
      }),
    )
    const element = await mount(client)
    expect(rowButton(element, 'daemon', 'Edit')).toBeDefined()
  })
})

describe('creating a user', () => {
  it('sends only the fields that were filled in', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    button(element, 'New user').click()
    await flush(element)

    setValue(input(element, 'input[data-field="name"]'), 'dana')
    setValue(input(element, 'input[data-field="gecos"]'), 'Dana Diaz')
    setValue(input(element, 'input[data-field="home"]'), '/home/dana')
    await element.updateComplete

    button(element, 'Create user').click()
    await flush(element)

    expect(client.calls).toEqual([
      {
        method: 'createUser',
        args: [
          {
            name: 'dana',
            uid: undefined,
            gid: undefined,
            gecos: 'Dana Diaz',
            home: '/home/dana',
            shell: undefined,
            system: false,
          },
        ],
      },
    ])
    expect(text(element)).toContain('created user dana')
  })

  it('reports a bad UID without calling the daemon', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    button(element, 'New user').click()
    await flush(element)
    setValue(input(element, 'input[data-field="name"]'), 'dana')
    setValue(input(element, 'input[data-field="uid"]'), '12x')
    await element.updateComplete

    button(element, 'Create user').click()
    await flush(element)

    expect(client.calls).toEqual([])
    expect(text(element)).toContain('UID must be a whole number.')
  })

  it('requires a name', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    button(element, 'New user').click()
    await flush(element)
    button(element, 'Create user').click()
    await flush(element)

    expect(client.calls).toEqual([])
    expect(text(element)).toContain('A user name is required.')
  })
})

describe('editing a user', () => {
  it('sends only the changed fields', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)

    setValue(input(element, 'input[data-field="shell"]'), '/bin/sh')
    await element.updateComplete
    button(element, 'Save changes').click()
    await flush(element)

    expect(client.calls[0].method).toBe('modifyUser')
    expect(client.calls[0].args).toEqual(['alice', { gecos: undefined, home: undefined, shell: '/bin/sh' }])
  })

  it('refuses a save that changes nothing', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)
    button(element, 'Save changes').click()
    await flush(element)

    expect(client.calls).toEqual([])
    expect(text(element)).toContain('Nothing changed.')
  })

  it('locks an account after confirmation', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)
    button(element, 'Lock').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(client.calls[0]).toEqual({ method: 'modifyUser', args: ['alice', { locked: true }] })
  })

  it('does nothing when the confirmation is declined', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)
    button(element, 'Lock').click()
    await flush(element)
    await answerDialogs('cancel')
    await flush(element)

    expect(client.calls).toEqual([])
  })

  it('deletes without touching the home directory by default', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)
    button(element, 'Delete').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(client.calls[0]).toEqual({ method: 'deleteUser', args: ['alice', false] })
  })

  it('passes remove_home when the box is ticked', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)

    const checkbox = [...(element.shadowRoot?.querySelectorAll('input[type=checkbox]') ?? [])][0] as HTMLInputElement
    checkbox.checked = true
    checkbox.dispatchEvent(new Event('change'))
    await element.updateComplete

    button(element, 'Delete').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(client.calls[0]).toEqual({ method: 'deleteUser', args: ['alice', true] })
  })
})

describe('setting a password', () => {
  async function openPasswordForm(enabled = true) {
    const client = new FakeAccountClient(
      accountSnapshot({ control_enabled: true, password_enabled: enabled }),
    )
    const element = await mount(client)
    rowButton(element, 'alice', 'Password')?.click()
    await flush(element)
    return { client, element }
  }

  it('sends the password only when both entries match', async () => {
    const { client, element } = await openPasswordForm()

    const fields = element.shadowRoot?.querySelectorAll('input[type=password]') ?? []
    setValue(fields[0] as HTMLInputElement, 'correct-horse')
    setValue(fields[1] as HTMLInputElement, 'correct-horse')
    await element.updateComplete

    button(element, 'Set password').click()
    await flush(element)

    expect(client.calls[0]).toEqual({
      method: 'setPassword',
      args: ['alice', 'correct-horse'],
    })
  })

  it('refuses a short password', async () => {
    const { client, element } = await openPasswordForm()

    const fields = element.shadowRoot?.querySelectorAll('input[type=password]') ?? []
    setValue(fields[0] as HTMLInputElement, 'short')
    setValue(fields[1] as HTMLInputElement, 'short')
    await element.updateComplete

    button(element, 'Set password').click()
    await flush(element)

    expect(client.calls).toEqual([])
    expect(text(element)).toContain('at least 8 characters')
  })

  it('refuses a mismatch', async () => {
    const { client, element } = await openPasswordForm()

    const fields = element.shadowRoot?.querySelectorAll('input[type=password]') ?? []
    setValue(fields[0] as HTMLInputElement, 'correct-horse')
    setValue(fields[1] as HTMLInputElement, 'correct-horsey')
    await element.updateComplete

    button(element, 'Set password').click()
    await flush(element)

    expect(client.calls).toEqual([])
    expect(text(element)).toContain('do not match')
  })
})

describe('groups', () => {
  async function mountGroups(snapshot = accountSnapshot()) {
    const client = new FakeAccountClient(snapshot)
    const element = await mount(client)
    button(element, 'Groups').click()
    await flush(element)
    return { client, element }
  }

  it('switches to the groups table', async () => {
    const { element } = await mountGroups(
      accountSnapshot({ groups: [accountGroup({ name: 'developers' })] }),
    )
    expect(text(element)).toContain('developers')
    expect(text(element)).toContain('Members')
  })

  it('marks a privileged group as administrative', async () => {
    const { element } = await mountGroups(
      accountSnapshot({ groups: [accountGroup({ name: 'sudo', gid: 27 })] }),
    )
    expect(text(element)).toContain('admin')
  })

  it('refuses to edit a privileged group without the opt-in', async () => {
    const { element } = await mountGroups(
      accountSnapshot({ groups: [accountGroup({ name: 'sudo', gid: 27 })] }),
    )
    expect(rowButton(element, 'sudo', 'Edit')).toBeUndefined()
    expect(text(element)).toContain('allow_privileged_groups')
  })

  it('allows editing a privileged group with the opt-in', async () => {
    const { element } = await mountGroups(
      accountSnapshot({
        allow_privileged_groups: true,
        groups: [accountGroup({ name: 'sudo', gid: 27 })],
      }),
    )
    expect(rowButton(element, 'sudo', 'Edit')).toBeDefined()
  })

  it('replaces the member list after confirmation', async () => {
    const { client, element } = await mountGroups(
      accountSnapshot({
        groups: [accountGroup({ name: 'developers', gid: 2000, members: ['alice'] })],
      }),
    )

    rowButton(element, 'developers', 'Edit')?.click()
    await flush(element)

    setValue(input(element, 'input[data-field="members"]'), ' bob , carol ')
    await element.updateComplete
    button(element, 'Save changes').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(client.calls[0]).toEqual({
      method: 'modifyGroup',
      args: ['developers', { gid: undefined, members: ['bob', 'carol'] }],
    })
  })

  it('sends an empty list to remove every member', async () => {
    const { client, element } = await mountGroups(
      accountSnapshot({
        groups: [accountGroup({ name: 'developers', gid: 2000, members: ['alice'] })],
      }),
    )

    rowButton(element, 'developers', 'Edit')?.click()
    await flush(element)
    setValue(input(element, 'input[data-field="members"]'), '')
    await element.updateComplete
    button(element, 'Save changes').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(client.calls[0]).toEqual({
      method: 'modifyGroup',
      args: ['developers', { gid: undefined, members: [] }],
    })
  })

  it('deletes a group after confirmation', async () => {
    const { client, element } = await mountGroups(
      accountSnapshot({ groups: [accountGroup({ name: 'developers' })] }),
    )

    rowButton(element, 'developers', 'Edit')?.click()
    await flush(element)
    button(element, 'Delete').click()
    await flush(element)
    await answerDialogs('confirm')
    await flush(element)

    expect(client.calls[0]).toEqual({ method: 'deleteGroup', args: ['developers'] })
  })

  it('creates a group with an optional gid', async () => {
    const { client, element } = await mountGroups()

    button(element, 'New group').click()
    await flush(element)
    setValue(input(element, 'input[data-field="group-name"]'), 'ops')
    setValue(input(element, 'input[data-field="gid"]'), '2001')
    await element.updateComplete

    button(element, 'Create group').click()
    await flush(element)

    expect(client.calls[0]).toEqual({
      method: 'createGroup',
      args: [{ name: 'ops', gid: 2001, system: false }],
    })
  })
})

describe('errors and refresh', () => {
  it('shows a daemon refusal and keeps the table', async () => {
    const client = new FakeAccountClient()
    client.error = new Error('the daemon refused: protected account')
    const element = await mount(client)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)
    setValue(input(element, 'input[data-field="shell"]'), '/bin/sh')
    await element.updateComplete
    button(element, 'Save changes').click()
    await flush(element)

    expect(text(element)).toContain('protected account')
    // The editor stays open so the operator can correct the input.
    expect(text(element)).toContain('alice')
  })

  it('refreshes after a successful mutation', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)
    expect(client.get).toHaveBeenCalledTimes(1)

    rowButton(element, 'alice', 'Edit')?.click()
    await flush(element)
    setValue(input(element, 'input[data-field="shell"]'), '/bin/sh')
    await element.updateComplete
    button(element, 'Save changes').click()
    await flush(element)

    expect(client.get).toHaveBeenCalledTimes(2)
  })

  it('issues exactly one request per mount, not one per update', async () => {
    const client = new FakeAccountClient()
    const element = await mount(client)
    await flush(element)
    await flush(element)
    expect(client.get).toHaveBeenCalledTimes(1)
  })
})
