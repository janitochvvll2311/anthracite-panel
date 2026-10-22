/**
 * The WireGuard panel and dashboard widget.
 *
 * Plain ES module with no build step: the host serves it from the bundle, and it
 * must not import anything from the panel's frontend. It talks only to its own
 * anchor namespace, derived from `import.meta.url`, so the same file works under
 * any mount point.
 */

const base = new URL('../', import.meta.url).pathname.replace(/\/$/, '')

async function call(path, options = {}) {
  const response = await fetch(`${base}${path}`, {
    credentials: 'same-origin',
    ...options,
    headers: {
      ...(options.body ? { 'content-type': 'application/json' } : {}),
      ...(options.headers ?? {}),
    },
  })
  const text = await response.text()
  let payload
  try {
    payload = text ? JSON.parse(text) : {}
  } catch {
    throw new Error(text || `HTTP ${response.status}`)
  }
  if (!response.ok) throw new Error(payload.error || `HTTP ${response.status}`)
  return payload
}

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (character) => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;',
  })[character])
}

function normalize(entry) {
  const name = entry.interface ?? entry.name ?? ''
  return {
    ...entry,
    name,
    up: entry.up === true,
    addresses: entry.addresses ?? [],
    peers: entry.peers ?? [],
  }
}

const STYLE = `
  :host { display: block; color: var(--text, #e6edf3); font: 13px/1.5 var(--mono, ui-monospace, SFMono-Regular, Menlo, monospace); }
  header { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin-bottom: 12px; }
  h3 { margin: 0; font-size: 14px; letter-spacing: 0.08em; text-transform: uppercase; }
  .spacer { flex: 1; }
  .status { color: var(--muted, #6b7b8c); }
  .error { color: var(--err, #f87171); margin: 0 0 10px; }
  .notice { color: var(--ok, #6ee7b7); margin: 0 0 10px; }
  .card { border: 1px solid var(--panel-border, #23303d); border-radius: 8px; padding: 14px; margin-bottom: 12px; background: var(--panel, #0e151d); }
  .card-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 8px; }
  .name { margin: 0; font-size: 14px; }
  .badge { font-size: 11px; padding: 1px 7px; border: 1px solid var(--panel-border, #23303d); border-radius: 999px; color: var(--muted, #8b9bad); }
  .badge[data-up='true'] { color: var(--ok, #6ee7b7); border-color: rgba(110, 231, 183, 0.4); }
  .meta { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 4px 12px; margin: 0 0 10px; }
  .meta div { min-width: 0; }
  .meta dt { color: var(--muted, #8b9bad); font-size: 11px; }
  .meta dd { margin: 0; overflow-wrap: anywhere; }
  table { border-collapse: collapse; width: 100%; margin-bottom: 10px; }
  th { text-align: left; color: var(--muted, #8b9bad); font-weight: 600; padding: 4px 8px 4px 0; border-bottom: 1px solid var(--panel-border, #23303d); }
  td { padding: 4px 8px 4px 0; border-bottom: 1px solid rgba(35, 48, 61, 0.5); vertical-align: top; }
  td.key { color: var(--accent, #7dd3fc); overflow-wrap: anywhere; }
  .empty { color: var(--muted, #6b7b8c); margin: 0; }
  form { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 8px; align-items: end; }
  label { display: grid; gap: 4px; color: var(--muted, #8b9bad); font-size: 11px; }
  input { background: var(--page-bg, #0a1118); border: 1px solid var(--panel-border, #23303d); border-radius: 6px; color: inherit; padding: 7px 9px; font: inherit; }
  input:focus { outline: 2px solid var(--accent, #2563eb); outline-offset: 1px; }
  button { background: var(--accent, #1d4ed8); border: 0; border-radius: 6px; color: #071018; padding: 7px 12px; font: inherit; font-weight: 600; cursor: pointer; }
  button.secondary { background: transparent; color: inherit; border: 1px solid var(--panel-border, #23303d); }
  button.danger { background: transparent; color: var(--err, #f87171); border: 1px solid rgba(248, 113, 113, 0.5); }
  button:disabled { opacity: 0.5; cursor: default; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 10px; }
  .form-title { margin: 0 0 8px; font-size: 13px; }
  @media (max-width: 640px) { form { grid-template-columns: 1fr; } }
`

class AnthWireguard extends HTMLElement {
  #interfaces = []
  #error = ''
  #status = ''

  constructor() {
    super()
    this.attachShadow({ mode: 'open' })
  }

  connectedCallback() {
    if (!this.shadowRoot.querySelector('style')) this.#render()
    this.refresh()
  }

  async refresh() {
    this.#status = 'Loading…'
    this.#paint()
    try {
      const payload = await call('/interfaces')
      this.#interfaces = (payload.interfaces ?? []).map(normalize)
      this.#error = ''
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#status = ''
      this.#paint()
    }
  }

  #paint() {
    const root = this.shadowRoot
    root.querySelector('[data-role="error"]').textContent = this.#error
    root.querySelector('[data-role="status"]').textContent = this.#status
    root.querySelector('[data-role="count"]').textContent = String(this.#interfaces.length)
    root.querySelector('[data-role="body"]').innerHTML = this.#interfaces.length
      ? this.#interfaces.map((entry) => `
          <section class="card">
            <div class="card-head">
              <h3 class="name">${escapeHtml(entry.name)}</h3>
              <span class="badge" data-up="${entry.up}">${entry.up ? 'up' : 'down'}</span>
              <span class="badge">${entry.peers.length} peer${entry.peers.length === 1 ? '' : 's'}</span>
            </div>
            <dl class="meta">
              <div><dt>Listen port</dt><dd>${escapeHtml(entry.listen_port ?? '—')}</dd></div>
              <div><dt>Addresses</dt><dd>${escapeHtml(entry.addresses.join(', ') || '—')}</dd></div>
            </dl>
          </section>`).join('')
      : '<p class="empty">No WireGuard interface is configured on this host.</p>'
  }

  #render() {
    const style = document.createElement('style')
    style.textContent = STYLE
    const shell = document.createElement('div')
    shell.innerHTML = `
      <header>
        <h3>WireGuard</h3>
        <span class="status"><span data-role="count">0</span> interfaces</span>
        <span class="spacer"></span>
        <span class="status" data-role="status"></span>
      </header>
      <p class="error" data-role="error"></p>
      <div data-role="body"></div>`
    this.shadowRoot.append(style, shell)
    this.#paint()
  }
}

class AnthWireguardPanel extends HTMLElement {
  #interfaces = []
  #error = ''
  #notice = ''
  #status = ''
  #busy = false
  #formMode = ''
  #formInterface = ''
  #draft = null
  #peerDraft = null

  constructor() {
    super()
    this.attachShadow({ mode: 'open' })
  }

  connectedCallback() {
    if (!this.shadowRoot.querySelector('style')) this.#render()
    this.refresh()
  }

  async refresh(preserveError = false) {
    if (this.#busy) return
    this.#busy = true
    this.#status = 'Loading…'
    this.#paint()
    try {
      const payload = await call('/interfaces')
      this.#interfaces = (payload.interfaces ?? []).map(normalize)
      if (!preserveError) this.#error = ''
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#busy = false
      this.#status = ''
      this.#paint()
    }
  }

  async #submitInterface(form) {
    const data = Object.fromEntries(new FormData(form))
    const interfaceName = String(data.interface ?? '').trim()
    const body = { interface: interfaceName }
    if (String(data.private_key ?? '').trim()) body.private_key = String(data.private_key).trim()
    if (String(data.listen_port ?? '').trim()) body.listen_port = Number(data.listen_port)
    if (String(data.addresses ?? '').trim()) {
      body.addresses = String(data.addresses).split(',').map((part) => part.trim()).filter(Boolean)
    }
    if (String(data.mtu ?? '').trim()) body.mtu = Number(data.mtu)

    this.#draft = data
    this.#formInterface = interfaceName
    this.#busy = true
    this.#error = ''
    this.#notice = ''
    try {
      if (this.#formMode === 'edit') {
        await call(`/interfaces/${encodeURIComponent(this.#formInterface)}`, {
          method: 'PATCH',
          body: JSON.stringify(body),
        })
        this.#notice = `Updated ${this.#formInterface}.`
      } else {
        await call('/interfaces', {
          method: 'POST',
          body: JSON.stringify(body),
        })
        this.#notice = `Created ${interfaceName}.`
      }
      this.#formMode = ''
      this.#draft = null
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#busy = false
      await this.refresh(true)
    }
  }

  async #submitPeer(form) {
    const data = Object.fromEntries(new FormData(form))
    const interfaceName = String(data.interface ?? '')
    const body = {
      interface: interfaceName,
      public_key: String(data.public_key ?? '').trim(),
      allowed_ips: String(data.allowed_ips ?? '').split(',').map((part) => part.trim()).filter(Boolean),
    }
    if (String(data.endpoint ?? '').trim()) body.endpoint = String(data.endpoint).trim()
    this.#peerDraft = { interface: interfaceName, ...data }
    this.#busy = true
    this.#error = ''
    this.#notice = ''
    try {
      await call('/peers', { method: 'POST', body: JSON.stringify(body) })
      this.#peerDraft = null
      this.#notice = `Added a peer to ${interfaceName}.`
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#busy = false
      await this.refresh(true)
    }
  }

  async #deleteInterface(interfaceName) {
    if (!confirm(`Delete ${interfaceName}? The config and interface will be removed.`)) return
    this.#busy = true
    this.#error = ''
    this.#notice = ''
    try {
      await call(`/interfaces/${encodeURIComponent(interfaceName)}`, { method: 'DELETE' })
      if (this.#formInterface === interfaceName) this.#formMode = ''
      this.#notice = `Deleted ${interfaceName}.`
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#busy = false
      await this.refresh(true)
    }
  }

  async #removePeer(interfaceName, publicKey) {
    if (!confirm('Remove this peer from the configuration and the running interface?')) return
    this.#busy = true
    this.#error = ''
    this.#notice = ''
    try {
      await call('/peers', {
        method: 'DELETE',
        body: JSON.stringify({ interface: interfaceName, public_key: publicKey }),
      })
      this.#notice = `Removed a peer from ${interfaceName}.`
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#busy = false
      await this.refresh(true)
    }
  }

  #interfaceForm() {
    if (!this.#formMode) return ''
    const editing = this.#formMode === 'edit'
    const entry = this.#interfaces.find((item) => item.name === this.#formInterface)
    const values = this.#draft ?? (editing && entry ? {
      interface: entry.name,
      listen_port: entry.listen_port ?? '',
      addresses: entry.addresses.join(', '),
      mtu: entry.mtu ?? '',
    } : {})
    const value = (key) => escapeHtml(values[key] ?? '')
    return `
      <section class="card">
        <h4 class="form-title">${editing ? `Edit ${escapeHtml(this.#formInterface)}` : 'New interface'}</h4>
        <form data-form="interface">
          <label>Name
            <input name="interface" required value="${value('interface')}" ${editing ? 'readonly' : ''} autocomplete="off" spellcheck="false" placeholder="wg0">
          </label>
          <label>Private key
            <input name="private_key" type="password" value="${value('private_key')}" autocomplete="new-password" spellcheck="false" placeholder="${editing ? 'Leave blank to keep' : 'Generate on host'}">
          </label>
          <label>Listen port
            <input name="listen_port" type="number" min="1" max="65535" value="${value('listen_port')}" placeholder="51820">
          </label>
          <label>Addresses
            <input name="addresses" value="${value('addresses')}" placeholder="10.0.0.1/24, fd00::1/64" autocomplete="off" spellcheck="false">
          </label>
          <label>MTU
            <input name="mtu" type="number" min="1280" max="65535" value="${value('mtu')}" placeholder="1420">
          </label>
          <div class="actions">
            <button type="submit" ${this.#busy ? 'disabled' : ''}>${editing ? 'Save' : 'Create'}</button>
            <button type="button" class="secondary" data-action="cancel">Cancel</button>
          </div>
        </form>
      </section>`
  }

  #peerForm(entry) {
    const draft = this.#peerDraft?.interface === entry.name ? this.#peerDraft : null
    const value = (key) => escapeHtml(draft?.[key] ?? '')
    return `
      <form data-form="peer">
        <input type="hidden" name="interface" value="${escapeHtml(entry.name)}">
        <label>Public key
          <input name="public_key" required value="${value('public_key')}" autocomplete="off" spellcheck="false">
        </label>
        <label>Allowed IPs
          <input name="allowed_ips" required value="${value('allowed_ips')}" placeholder="10.0.0.2/32" autocomplete="off" spellcheck="false">
        </label>
        <label>Endpoint
          <input name="endpoint" value="${value('endpoint')}" placeholder="host:51820" autocomplete="off" spellcheck="false">
        </label>
        <button type="submit" ${this.#busy ? 'disabled' : ''}>Add peer</button>
      </form>`
  }

  #body() {
    if (this.#interfaces.length === 0 && !this.#formMode) {
      return '<p class="empty">No WireGuard interface is configured on this host.</p>'
    }
    return this.#interfaces.map((entry) => {
      if (entry.error) {
        return `<section class="card"><h3 class="name">${escapeHtml(entry.name)}</h3><p class="error">${escapeHtml(entry.error)}</p></section>`
      }
      const peers = entry.peers.length
        ? `<table><thead><tr><th>Public key</th><th>Allowed IPs</th><th>Endpoint</th><th></th></tr></thead><tbody>
            ${entry.peers.map((peer) => `<tr>
              <td class="key">${escapeHtml(peer.public_key ?? '')}</td>
              <td>${escapeHtml((peer.allowed_ips ?? []).join(', '))}</td>
              <td>${escapeHtml(peer.endpoint ?? '—')}</td>
              <td><button type="button" class="danger" data-action="remove-peer" data-interface="${escapeHtml(entry.name)}" data-key="${escapeHtml(peer.public_key ?? '')}">Remove</button></td>
            </tr>`).join('')}
          </tbody></table>`
        : '<p class="empty">No peer is configured.</p>'
      return `
        <section class="card">
          <div class="card-head">
            <h3 class="name">${escapeHtml(entry.name)}</h3>
            <span class="badge" data-up="${entry.up}">${entry.up ? 'up' : 'down'}</span>
            <span class="spacer"></span>
            <button type="button" class="secondary" data-action="edit" data-interface="${escapeHtml(entry.name)}">Edit</button>
            <button type="button" class="danger" data-action="delete" data-interface="${escapeHtml(entry.name)}">Delete</button>
          </div>
          <dl class="meta">
            <div><dt>Public key</dt><dd>${escapeHtml(entry.public_key ?? '—')}</dd></div>
            <div><dt>Listen port</dt><dd>${escapeHtml(entry.listen_port ?? '—')}</dd></div>
            <div><dt>Addresses</dt><dd>${escapeHtml(entry.addresses.join(', ') || '—')}</dd></div>
            <div><dt>MTU</dt><dd>${escapeHtml(entry.mtu ?? '—')}</dd></div>
          </dl>
          ${peers}
          ${this.#peerForm(entry)}
        </section>`
    }).join('')
  }

  #paint() {
    const root = this.shadowRoot
    root.querySelector('[data-role="error"]').textContent = this.#error
    root.querySelector('[data-role="notice"]').textContent = this.#notice
    root.querySelector('[data-role="status"]').textContent = this.#status
    root.querySelector('[data-role="body"]').innerHTML =
      this.#interfaceForm() + this.#body()
  }

  #render() {
    const style = document.createElement('style')
    style.textContent = STYLE
    const shell = document.createElement('div')
    shell.innerHTML = `
      <header>
        <h3>WireGuard</h3>
        <span class="status" data-role="status"></span>
        <span class="spacer"></span>
        <button type="button" data-action="new" ${this.#busy ? 'disabled' : ''}>New interface</button>
      </header>
      <p class="error" data-role="error"></p>
      <p class="notice" data-role="notice"></p>
      <div data-role="body"></div>`
    this.shadowRoot.append(style, shell)

    // The header (with "New interface") and the body are both inside `shell`.
    // Listening on `body` missed the header button because `#paint` only
    // replaces the body's innerHTML; the shell is the stable common ancestor.
    shell.addEventListener('click', (event) => {
      const button = event.target.closest('[data-action]')
      if (!button) return
      const action = button.dataset.action
      if (action === 'new') {
        this.#formMode = 'create'
        this.#formInterface = ''
        this.#draft = null
        this.#paint()
      } else if (action === 'edit') {
        this.#formMode = 'edit'
        this.#formInterface = button.dataset.interface ?? ''
        this.#draft = null
        this.#paint()
      } else if (action === 'cancel') {
        this.#formMode = ''
        this.#formInterface = ''
        this.#draft = null
        this.#paint()
      } else if (action === 'delete') {
        this.#deleteInterface(button.dataset.interface ?? '')
      } else if (action === 'remove-peer') {
        this.#removePeer(button.dataset.interface ?? '', button.dataset.key ?? '')
      }
    })
    shell.addEventListener('submit', (event) => {
      const form = event.target
      if (!(form instanceof HTMLFormElement)) return
      event.preventDefault()
      if (form.dataset.form === 'peer') this.#submitPeer(form)
      else this.#submitInterface(form)
    })
    this.#paint()
  }
}

if (!customElements.get('anth-wireguard')) {
  customElements.define('anth-wireguard', AnthWireguard)
}
if (!customElements.get('anth-wireguard-panel')) {
  customElements.define('anth-wireguard-panel', AnthWireguardPanel)
}
