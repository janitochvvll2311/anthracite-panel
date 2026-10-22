import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import type {
  PluginClient,
  PluginInstallResult,
  PluginsCatalogue,
} from '../lib/plugins/plugin-client'
import { confirmAction } from '../lib/ui/dialog'

/**
 * Installed plugin bundles and the browser install path.
 *
 * Like the other managers it depends on an injected `PluginClient`, so the
 * component never reaches `/api/plugins` itself. Installation is the only
 * mutating action and it is deliberately behind a confirmation that names the
 * consequence: a bundle runs code with the daemon's privileges.
 */
@customElement('anth-plugin-manager')
export class AnthPluginManager extends LitElement {
  @property({ attribute: false })
  client?: PluginClient

  @property({ type: Boolean, attribute: 'auto-load' })
  autoLoad = true

  @state()
  private catalogue?: PluginsCatalogue

  @state()
  private loading = false

  @state()
  private busy = false

  @state()
  private errorMessage = ''

  @state()
  private notice = ''

  @state()
  private warning = ''

  @state()
  private selected?: File

  protected override willUpdate(changed: PropertyValues<this>): void {
    if (
      this.client &&
      this.autoLoad &&
      (changed.has('client') || (changed.has('autoLoad') && this.autoLoad))
    ) {
      void this.refresh()
    }
  }

  /** Reads the catalogue without changing anything. */
  async refresh(): Promise<void> {
    if (!this.client || this.loading || this.busy) return

    this.loading = true
    this.errorMessage = ''
    try {
      this.catalogue = await this.client.list()
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.loading = false
    }
  }

  private get canInstall(): boolean {
    return (
      this.catalogue?.enabled === true &&
      this.catalogue.install_from_web === true
    )
  }

  private get installReason(): string {
    if (!this.catalogue) return 'The catalogue has not loaded yet.'
    if (!this.catalogue.enabled) {
      return 'Plugins are disabled by [plugins] enabled.'
    }
    if (!this.catalogue.install_from_web) {
      return 'Browser installation is disabled by [plugins] install_from_web.'
    }
    return ''
  }

  private onFileChange(event: Event): void {
    const input = event.target as HTMLInputElement
    this.selected = input.files?.[0]
    this.errorMessage = ''
    this.notice = ''
    this.warning = ''
  }

  private async install(event: Event): Promise<void> {
    event.preventDefault()
    if (!this.client || !this.selected || !this.canInstall || this.busy) return

    const file = this.selected
    const accepted = await confirmAction({
      title: 'Install this plugin bundle?',
      message:
        `${file.name} will run code with the daemon's privileges. ` +
        'Only install a bundle whose author and signature you trust.',
      confirmLabel: 'Install plugin',
      danger: true,
    })
    if (!accepted) return

    this.busy = true
    this.errorMessage = ''
    this.notice = ''
    this.warning = ''
    let installed = false
    try {
      const result: PluginInstallResult = await this.client.install(file)
      this.notice = `Installed ${result.name} ${result.version}.`
      this.warning = result.warning ?? ''
      this.selected = undefined
      installed = true
      const input = this.renderRoot.querySelector<HTMLInputElement>(
        'input[type="file"]',
      )
      if (input) input.value = ''
    } catch (error) {
      this.errorMessage = messageOf(error)
    } finally {
      this.busy = false
    }
    // Refresh only after a success: a failed install must keep its error on
    // screen, and `refresh` clears the error before reading.
    if (installed) await this.refresh()
  }

  override render() {
    const catalogue = this.catalogue
    return html`
      <section class="panel">
        <header class="toolbar">
          <div>
            <h2>Plugins</h2>
            <p class="muted">
              Bundles installed on this host, their frozen capabilities and the
              web install switch.
            </p>
          </div>
          <button
            class="secondary"
            type="button"
            ?disabled=${!this.client || this.loading || this.busy}
            @click=${() => this.refresh()}
          >
            Refresh
          </button>
        </header>

        ${this.errorMessage
          ? html`<p class="error" role="alert">${this.errorMessage}</p>`
          : nothing}
        ${this.notice
          ? html`<p class="notice" role="status">${this.notice}</p>`
          : nothing}
        ${this.warning
          ? html`<p class="warning" role="status">${this.warning}</p>`
          : nothing}

        <section class="install" aria-label="Install a plugin bundle">
          <h3>Install from the browser</h3>
          <p class="muted">
            A bundle is a zip with <code>plugin.toml</code>,
            <code>plugin.wasm</code> and optionally <code>plugin.sig</code>.
            Installing runs the plugin's code with the daemon's privileges.
          </p>
          ${this.canInstall
            ? html`
                <form @submit=${this.install}>
                  <input
                    type="file"
                    accept=".zip,application/zip"
                    @change=${this.onFileChange}
                  />
                  <button
                    class="danger"
                    type="submit"
                    ?disabled=${!this.selected || this.busy}
                  >
                    ${this.busy ? 'Installing…' : 'Install bundle'}
                  </button>
                </form>
              `
            : html`<p class="muted">${this.installReason}</p>`}
        </section>

        ${catalogue?.enabled === false
          ? html`<p class="muted">Plugins are switched off.</p>`
          : nothing}
        ${catalogue?.problems.length
          ? html`
              <section class="problems" aria-label="Plugin problems">
                <h3>Problems</h3>
                <ul>
                  ${catalogue.problems.map((problem) => html`<li>${problem}</li>`)}
                </ul>
              </section>
            `
          : nothing}

        <section class="installed" aria-label="Installed plugins">
          <h3>
            Installed
            ${catalogue
              ? html`<span class="count">${catalogue.plugins.length}</span>`
              : nothing}
          </h3>
          ${catalogue && catalogue.plugins.length === 0
            ? html`<p class="muted">No plugins are installed.</p>`
            : nothing}
          <ul class="plugin-list">
            ${catalogue?.plugins.map(
              (plugin) => html`
                <li class="plugin">
                  <div class="plugin-head">
                    <span class="plugin-name">${plugin.name}</span>
                    <span class="plugin-id">${plugin.id}</span>
                    <span class="version">v${plugin.version}</span>
                    <span class="state ${plugin.enabled ? 'on' : 'off'}">
                      ${plugin.enabled ? 'enabled' : 'disabled'}
                    </span>
                  </div>
                  ${plugin.capabilities.length
                    ? html`
                        <ul class="chips" aria-label="Capabilities">
                          ${plugin.capabilities.map(
                            (capability) =>
                              html`<li class="chip">${capability}</li>`,
                          )}
                        </ul>
                      `
                    : html`<p class="muted">No capabilities frozen.</p>`}
                  ${plugin.ui.length
                    ? html`<p class="muted">UI: ${plugin.ui.length}</p>`
                    : nothing}
                  ${plugin.api.length
                    ? html`<p class="muted">API: ${plugin.api.length}</p>`
                    : nothing}
                </li>
              `,
            )}
          </ul>
        </section>
      </section>
    `
  }

  static override styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 14px/1.5 var(--sans, system-ui, sans-serif);
    }

    .panel {
      display: grid;
      gap: 1rem;
      padding: 1rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 12px;
      background: var(--panel, #101721);
    }

    .toolbar,
    .plugin-head {
      display: flex;
      flex-wrap: wrap;
      gap: 0.6rem;
      align-items: center;
      justify-content: space-between;
    }

    h2,
    h3 {
      margin: 0;
      font: 600 15px/1.3 var(--mono, ui-monospace, monospace);
    }

    h3 {
      font-size: 13px;
    }

    .muted {
      margin: 0;
      color: var(--muted, #8b9bad);
      font-size: 13px;
    }

    code {
      font-family: var(--mono, ui-monospace, monospace);
      color: var(--accent, #7dd3fc);
    }

    button {
      padding: 0.4rem 0.7rem;
      color: inherit;
      font: 600 12px/1.2 var(--mono, monospace);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: #16202c;
      cursor: pointer;
    }

    button:disabled {
      opacity: 0.55;
      cursor: not-allowed;
    }

    button.danger {
      border-color: rgba(248, 113, 113, 0.55);
      color: #fca5a5;
    }

    button.secondary:hover:not(:disabled) {
      border-color: rgba(125, 211, 252, 0.55);
    }

    .install,
    .installed,
    .problems {
      display: grid;
      gap: 0.6rem;
      padding: 0.9rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: rgba(7, 10, 15, 0.35);
    }

    form {
      display: flex;
      flex-wrap: wrap;
      gap: 0.6rem;
      align-items: center;
    }

    input[type='file'] {
      color: inherit;
      font: 12px/1.4 var(--mono, monospace);
    }

    .error,
    .notice,
    .warning {
      margin: 0;
      padding: 0.6rem 0.75rem;
      border-radius: 8px;
      font-size: 13px;
    }

    .error {
      color: #fecaca;
      background: rgba(127, 29, 29, 0.35);
    }

    .notice {
      color: #bbf7d0;
      background: rgba(20, 83, 45, 0.35);
    }

    .warning {
      color: #fde68a;
      background: rgba(113, 63, 18, 0.35);
    }

    .plugin-list,
    .problems ul {
      display: grid;
      gap: 0.5rem;
      margin: 0;
      padding: 0;
      list-style: none;
    }

    .plugin {
      display: grid;
      gap: 0.35rem;
      padding: 0.7rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
    }

    .plugin-name {
      font-weight: 600;
    }

    .plugin-id,
    .version,
    .state,
    .count {
      color: var(--muted, #8b9bad);
      font: 11px/1.3 var(--mono, monospace);
    }

    .state.on {
      color: #86efac;
    }

    .state.off {
      color: #fca5a5;
    }

    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: 0.35rem;
      margin: 0;
      padding: 0;
      list-style: none;
    }

    .chip {
      padding: 0.1rem 0.4rem;
      color: var(--accent, #7dd3fc);
      font: 11px/1.3 var(--mono, monospace);
      border: 1px solid rgba(125, 211, 252, 0.3);
      border-radius: 999px;
    }
  `
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-plugin-manager': AnthPluginManager
  }
}
