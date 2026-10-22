/**
 * The WireGuard panel, shipped inside the plugin bundle.
 *
 * Plain ES module with no build step: the host serves it straight from the
 * bundle, and it must not import anything from the panel's own frontend, which
 * is a separate origin path with a separate version. Everything it needs is in
 * this file.
 *
 * The component talks only to its own anchor namespace, which it derives from
 * its own module URL, so the same file works under any mount point the host
 * chooses and needs no configuration injected.
 */

// `import.meta.url` is `/api/plugins/<id>/web/wireguard.js`; relative resolution
// starts from its directory (`.../web/`), so one level up is the anchor
// namespace `/api/plugins/<id>`. Deriving it keeps the file mount-point
// independent: the host can rename the route without editing this.
const base = new URL('../', import.meta.url).pathname.replace(/\/$/, '')

class AnthWireguard extends HTMLElement {
  #interfaces = []
  #error = ''
  #busy = false
  #status = ''

  constructor() {
    super()
    this.attachShadow({ mode: 'open' })
  }

  connectedCallback() {
    if (!this.shadowRoot.querySelector('style')) this.#render()
    this.refresh()
  }

  async #call(path, options = {}) {
    const response = await fetch(`${base}${path}`, {
      headers: { 'content-type': 'application/json' },
      credentials: 'same-origin',
      ...options,
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

  async refresh() {
    if (this.#busy) return
    this.#busy = true
    this.#status = 'Loading…'
    this.#paint()
    try {
      const payload = await this.#call('/interfaces')
      this.#interfaces = payload.interfaces ?? []
      this.#error = ''
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#busy = false
      this.#status = ''
      this.#paint()
    }
  }

  async #addPeer(event) {
    event.preventDefault()
    const form = event.currentTarget
    const data = new FormData(form)
    const interfaceName = data.get('interface')
    const publicKey = (data.get('public_key') ?? '').trim()
    const allowed = (data.get('allowed_ips') ?? '').trim()
    if (!interfaceName || !publicKey || !allowed) {
      this.#error = 'Interface, public key and allowed IPs are all required.'
      this.#paint()
      return
    }
    this.#busy = true
    this.#status = 'Adding…'
    this.#paint()
    try {
      await this.#call('/peers', {
        method: 'POST',
        body: JSON.stringify({
          interface: interfaceName,
          public_key: publicKey,
          allowed_ips: allowed.split(',').map((part) => part.trim()).filter(Boolean),
          endpoint: (data.get('endpoint') ?? '').trim() || undefined,
        }),
      })
      form.reset()
      this.#error = ''
    } catch (error) {
      this.#error = error.message
    } finally {
      this.#busy = false
      this.#status = ''
      await this.refresh()
    }
  }

  #paint() {
    const root = this.shadowRoot
    const body = root.querySelector('[data-role="body"]')
    // The body is replaced wholesale on every paint, so the listener below is
    // attached to a fresh form each time. Delegating from the body instead would
    // keep one listener, but then a submit that repaints mid-flight would fire
    // twice; rebuilding is the cheaper correctness here.
    body.innerHTML = this.#body()
    root.querySelector('[data-role="error"]').textContent = this.#error
    root.querySelector('[data-role="status"]').textContent = this.#status
    root.querySelector('[data-role="interfaces"]').textContent = String(this.#interfaces.length)
    body.querySelector('form')?.addEventListener('submit', (event) => this.#addPeer(event))
  }

  #body() {
    if (this.#interfaces.length === 0) {
      return '<p class="empty">No WireGuard interface is configured on this host.</p>'
    }
    return this.#interfaces.map((entry) => this.#interface(entry)).join('')
  }

  #interface(entry) {
    if (entry.error) {
      return `<section class="card"><h3>${escapeHtml(entry.name)}</h3>
        <p class="error">${escapeHtml(entry.error)}</p></section>`
    }
    const peers = (entry.peers ?? []).map((peer) => `
      <tr>
        <td class="key">${escapeHtml(peer.public_key ?? '')}</td>
        <td>${escapeHtml((peer.allowed_ips ?? []).join(', '))}</td>
        <td>${escapeHtml(peer.latest_handshake ?? 'never')}</td>
        <td>${escapeHtml(peer.received ?? '—')} / ${escapeHtml(peer.sent ?? '—')}</td>
      </tr>`).join('')
    return `<section class="card">
      <h3>${escapeHtml(entry.name)}${entry.listen_port ? `<span class="port">:${entry.listen_port}</span>` : ''}</h3>
      ${peers ? `<table><thead><tr>
        <th>Public key</th><th>Allowed IPs</th><th>Latest handshake</th><th>Received / sent</th>
      </tr></thead><tbody>${peers}</tbody></table>`
        : '<p class="empty">No peer is configured.</p>'}
      <form>
        <input type="hidden" name="interface" value="${escapeHtml(entry.name)}">
        <label>Public key <input name="public_key" required autocomplete="off" spellcheck="false"></label>
        <label>Allowed IPs <input name="allowed_ips" required placeholder="10.0.0.2/32" autocomplete="off" spellcheck="false"></label>
        <label>Endpoint <input name="endpoint" placeholder="host:51820" autocomplete="off" spellcheck="false"></label>
        <button type="submit">Add peer</button>
      </form>
    </section>`
  }

  #render() {
    const style = document.createElement('style')
    style.textContent = `
      :host { display: block; color: #e6edf3; font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
      header { display: flex; align-items: baseline; gap: 12px; margin-bottom: 12px; }
      h3 { margin: 0 0 8px; font-size: 14px; letter-spacing: 0.08em; text-transform: uppercase; }
      .port { color: #6ee7b7; margin-left: 6px; }
      .card { border: 1px solid #23303d; border-radius: 8px; padding: 14px; margin-bottom: 12px; background: #0e151d; }
      .empty { color: #6b7b8c; margin: 0; }
      .error { color: #f87171; margin: 0 0 8px; }
      table { border-collapse: collapse; width: 100%; margin-bottom: 12px; }
      th { text-align: left; color: #8b9bad; font-weight: 600; padding: 4px 8px 4px 0; border-bottom: 1px solid #23303d; }
      td { padding: 4px 8px 4px 0; border-bottom: 1px solid #16202a; }
      td.key { color: #7dd3fc; }
      form { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 8px; align-items: end; }
      label { display: grid; gap: 4px; color: #8b9bad; }
      input { background: #0a1118; border: 1px solid #23303d; border-radius: 6px; color: inherit; padding: 6px 8px; font: inherit; }
      input:focus { outline: 2px solid #2563eb; outline-offset: 1px; }
      button { background: #1d4ed8; border: 0; border-radius: 6px; color: #fff; padding: 7px 14px; font: inherit; cursor: pointer; }
      button:disabled { opacity: 0.5; cursor: default; }
      [data-role="status"] { color: #6b7b8c; }
    `
    const shell = document.createElement('div')
    shell.innerHTML = `
      <header>
        <h3>WireGuard</h3>
        <span><span data-role="interfaces">0</span> interfaces</span>
        <span data-role="status"></span>
      </header>
      <p class="error" data-role="error"></p>
      <div data-role="body"></div>
    `
    this.shadowRoot.append(style, shell)
    this.#paint()
  }
}

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (character) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  })[character])
}

if (!customElements.get('anth-wireguard')) {
  customElements.define('anth-wireguard', AnthWireguard)
}
