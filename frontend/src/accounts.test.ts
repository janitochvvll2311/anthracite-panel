import { afterEach, expect, test, vi } from 'vitest'

import './accounts'
import { HttpAccountClient } from './accounts-client'
import type {
  AccountClient,
  AccountGroup,
  AccountSnapshot,
  AccountUser,
} from './accounts-client'

function user(overrides: Partial<AccountUser> = {}): AccountUser {
  return {
    name: 'alice',
    uid: 1001,
    gid: 1001,
    group: 'alice',
    gecos: 'Alice',
    home: '/home/alice',
    shell: '/bin/sh',
    locked: false,
    system: false,
    source: 'local',
    groups: ['wheel'],
    ...overrides,
  }
}

function group(overrides: Partial<AccountGroup> = {}): AccountGroup {
  return {
    name: 'builders',
    gid: 1500,
    members: ['alice'],
    system: false,
    source: 'local',
    ...overrides,
  }
}

function snapshot(overrides: Partial<AccountSnapshot> = {}): AccountSnapshot {
  return {
    users: [
      user(),
      user({ name: 'root', uid: 0, gid: 0, group: 'root', system: true, groups: [] }),
      user({ name: 'bob', uid: 1002, gid: 1002, group: 'bob', locked: undefined, groups: [] }),
      user({
        name: 'dora',
        uid: 2001,
        gid: 2001,
        group: 'dora',
        source: 'nss',
        locked: undefined,
        groups: [],
      }),
    ],
    groups: [
      group(),
      group({ name: 'root', gid: 0, system: true, members: [] }),
      group({ name: 'docker', gid: 999, system: true, members: [] }),
      group({ name: 'dora', gid: 2001, source: 'nss', members: [] }),
    ],
    control_enabled: true,
    password_enabled: true,
    allow_system: true,
    allow_privileged_groups: true,
    timestamp_ms: 1,
    ...overrides,
  }
}

type AccountsElement = HTMLElement & {
  client: AccountClient
  view: 'users' | 'groups'
  updateComplete: Promise<boolean>
}

async function settle(element: AccountsElement): Promise<void> {
  for (let index = 0; index < 6; index += 1) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await element.updateComplete
  }
}

function button(container: ParentNode, label: string): HTMLButtonElement {
  const found = [...container.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!found) throw new Error(`missing button: ${label}`)
  return found
}

async function mountAccounts(
  client: AccountClient,
  view: 'users' | 'groups' = 'users',
): Promise<AccountsElement> {
  const element = document.createElement('anth-accounts') as AccountsElement
  element.client = client
  element.view = view
  document.body.append(element)
  await settle(element)
  return element
}

function fakeClient(value = snapshot()) {
  const get = vi.fn().mockResolvedValue(value)
  const createUser = vi.fn().mockResolvedValue('created user carol')
  const deleteUser = vi.fn().mockResolvedValue('deleted user bob')
  const setPassword = vi.fn().mockResolvedValue('set the password for bob')
  const createGroup = vi.fn().mockResolvedValue('created group devs')
  const deleteGroup = vi.fn().mockResolvedValue('deleted group builders')
  return {
    client: {
      get,
      createUser,
      deleteUser,
      setPassword,
      createGroup,
      deleteGroup,
    } as AccountClient,
    get,
    createUser,
    deleteUser,
    setPassword,
    createGroup,
    deleteGroup,
  }
}

function card(element: AccountsElement, name: string): HTMLElement {
  const found = [...(element.shadowRoot?.querySelectorAll<HTMLElement>('.user') ?? [])].find(
    (candidate) => candidate.querySelector('h3')?.textContent === name,
  )
  if (!found) throw new Error(`missing card: ${name}`)
  return found
}

afterEach(() => {
  document.body.replaceChildren()
})

test('the users view renders the inventory, policy and create form', async () => {
  const feed = fakeClient()
  const element = await mountAccounts(feed.client)
  const root = element.shadowRoot!

  expect(root.querySelector('.count')?.textContent).toContain('4 users · 1 system')
  expect(root.querySelectorAll('.user')).toHaveLength(4)
  expect(card(element, 'alice').textContent).toContain('uid 1001')
  expect(card(element, 'alice').textContent).toContain('locked')
  expect(card(element, 'alice').textContent).toContain('wheel')
  expect(card(element, 'bob').textContent).toContain('lock unknown')
  expect(root.querySelector('[part="create-user"]')).not.toBeNull()
  expect(root.querySelector('.policies')?.textContent).toContain('passwords')
})

test('creates a user with ids, description and system flag', async () => {
  const feed = fakeClient()
  const element = await mountAccounts(feed.client)
  const form = element.shadowRoot?.querySelector<HTMLFormElement>('[part="create-user"]')!
  const set = (name: string, value: string) => {
    const input = form.querySelector<HTMLInputElement>(`[name="${name}"]`)!
    input.value = value
    input.dispatchEvent(new Event('input'))
  }
  set('name', 'carol')
  set('uid', '1500')
  set('gid', '1500')
  set('gecos', 'Carol')
  set('home', '/home/carol')
  set('shell', '/bin/bash')
  const system = form.querySelector<HTMLInputElement>('[name="system"]')!
  system.checked = true
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)

  expect(feed.createUser).toHaveBeenCalledWith({
    name: 'carol',
    uid: 1500,
    gid: 1500,
    gecos: 'Carol',
    home: '/home/carol',
    shell: '/bin/bash',
    system: true,
  })
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
})

test('sets a password with confirmation and rejects a mismatch', async () => {
  const feed = fakeClient()
  const element = await mountAccounts(feed.client)
  const alice = card(element, 'alice')

  button(alice, 'Password').click()
  await element.updateComplete
  const form = card(element, 'alice').querySelector<HTMLFormElement>('.manage.inline')!
  const password = form.querySelector<HTMLInputElement>('[name="password"]')!
  const confirm = form.querySelector<HTMLInputElement>('[name="confirm"]')!
  password.value = 'long enough'
  confirm.value = 'different'
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await element.updateComplete
  expect(element.shadowRoot?.textContent).toContain('The passwords do not match.')
  expect(feed.setPassword).not.toHaveBeenCalled()

  confirm.value = 'long enough'
  form.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)
  expect(feed.setPassword).toHaveBeenCalledWith('alice', 'long enough')
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
})

test('deletes a user only after a confirmation that can remove the home', async () => {
  const feed = fakeClient()
  const element = await mountAccounts(feed.client)
  const bob = card(element, 'bob')
  button(bob, 'Delete').click()
  await element.updateComplete

  const confirm = card(element, 'bob').querySelector<HTMLElement>('.confirm')!
  expect(confirm.textContent).toContain('Delete user bob?')
  const removeHome = confirm.querySelector<HTMLInputElement>('input[type="checkbox"]')!
  removeHome.checked = true
  removeHome.dispatchEvent(new Event('change'))
  button(confirm, 'Delete').click()
  await settle(element)

  expect(feed.deleteUser).toHaveBeenCalledWith({ name: 'bob', remove_home: true })
  expect(element.shadowRoot?.textContent).toContain('Change applied.')
})

test('creates and deletes groups from the groups view', async () => {
  const feed = fakeClient()
  const element = await mountAccounts(feed.client, 'groups')
  const root = element.shadowRoot!

  expect(root.querySelector('.count')?.textContent).toContain('4 groups · 2 system')
  const create = root.querySelector<HTMLFormElement>('[part="create-group"]')!
  const name = create.querySelector<HTMLInputElement>('[name="name"]')!
  name.value = 'devs'
  create.dispatchEvent(new Event('submit', { cancelable: true }))
  await settle(element)
  expect(feed.createGroup).toHaveBeenCalledWith({ name: 'devs', system: false })

  const builders = card(element, 'builders')
  button(builders, 'Delete').click()
  await element.updateComplete
  const confirm = card(element, 'builders').querySelector<HTMLElement>('.confirm')!
  button(confirm, 'Delete').click()
  await settle(element)
  expect(feed.deleteGroup).toHaveBeenCalledWith('builders')
})

test('read-only and disabled entries offer no actions', async () => {
  const feed = fakeClient(snapshot({ control_enabled: false }))
  const element = await mountAccounts(feed.client)
  const root = element.shadowRoot!
  expect(root.querySelector('[part="create-user"]')).toBeNull()
  expect(root.querySelector('.user .actions button')).toBeNull()
  expect(root.textContent).toContain('Account changes are disabled')

  const pass = fakeClient(
    snapshot({ control_enabled: true, password_enabled: false, allow_system: false }),
  )
  const limited = await mountAccounts(pass.client)
  expect(card(limited, 'root').querySelector('.actions button')).toBeNull()
  expect(card(limited, 'dora').querySelector('.actions button')).toBeNull()
  expect(
    [...card(limited, 'alice').querySelectorAll('.actions button')].map((node) =>
      node.textContent?.trim(),
    ),
  ).toEqual(['Delete'])
})

test('the http client calls every account route with the right body', async () => {
  const calls: Array<{ url: string; method?: string; body?: unknown }> = []
  const fetchImpl = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({
      url: String(url),
      method: init?.method,
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    })
    return new Response(JSON.stringify({ message: 'ok' }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  }) as unknown as typeof fetch
  const client = new HttpAccountClient(fetchImpl)

  await expect(client.createUser({ name: 'carol', system: false })).resolves.toBe('ok')
  await expect(
    client.deleteUser({ name: 'bob', remove_home: true }),
  ).resolves.toBe('ok')
  await expect(client.setPassword('bob', 'long enough')).resolves.toBe('ok')
  await expect(client.createGroup({ name: 'devs', system: false })).resolves.toBe('ok')
  await expect(client.deleteGroup('builders')).resolves.toBe('ok')

  expect(calls.map((call) => call.url)).toEqual([
    '/api/accounts/users',
    '/api/accounts/users/delete',
    '/api/accounts/users/password',
    '/api/accounts/groups',
    '/api/accounts/groups/delete',
  ])
  expect(calls.map((call) => call.body)).toEqual([
    { name: 'carol', system: false },
    { name: 'bob', remove_home: true },
    { name: 'bob', password: 'long enough' },
    { name: 'devs', system: false },
    { name: 'builders' },
  ])
})
