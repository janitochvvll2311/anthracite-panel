/**
 * `anth-maintenance`: restarting the panel, and rebooting the host.
 *
 * Two buttons on one page and two paragraphs explaining what each costs, because
 * the cost is the whole difference. A page that offered one "restart" would make
 * "my settings did not apply" and "the machine went down" look like the same
 * click.
 *
 * The element owns the confirmation and the client, and the shell only mounts it.
 * A test mounts it with a plain object as the client, so no assertion here needs
 * a port or a session.
 */

import { WireError } from "@anth/wire";

import { escape } from "./html";
import type { MaintenanceClient } from "./maintenance-client";

const STYLE = `
:host { display: block; }
.card { background: var(--panel); border: 1px solid var(--panel-border);
  border-radius: 10px; padding: 14px 16px; margin: 0 0 14px; }
.card h2 { font-size: 0.95rem; margin: 0 0 4px; }
.card p { color: var(--muted); font-size: 0.82rem; margin: 0 0 12px; line-height: 1.5; }
.cost { color: var(--muted); font-size: 0.78rem; margin: 0 0 12px; padding-left: 18px; }
.cost li { margin: 2px 0; }
.row { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
button { font: inherit; font-size: 0.82rem; padding: 6px 12px; border-radius: 6px;
  border: 1px solid var(--panel-border); background: var(--panel); color: var(--text);
  cursor: pointer; }
button:hover:not([disabled]) { border-color: var(--accent); }
button[disabled] { opacity: 0.55; cursor: not-allowed; }
button.danger { border-color: var(--err); color: var(--err); }
label { font-size: 0.78rem; color: var(--muted); display: inline-flex; gap: 6px;
  align-items: center; }
input { font: inherit; font-size: 0.8rem; width: 4.5rem; padding: 4px 6px;
  border-radius: 5px; border: 1px solid var(--panel-border);
  background: var(--page-bg); color: var(--text); }
.said { font-size: 0.8rem; margin: 10px 0 0; min-height: 1.2em; }
.said.ok { color: var(--ok); }
.said.err { color: var(--err); }
`;

export class AnthMaintenance extends HTMLElement {
  /** Injected by the shell, or by a test that never binds a port. */
  static client: MaintenanceClient | undefined;

  #busy = false;
  #said = "";
  #saidIsError = false;

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    this.#paint();
    this.shadowRoot?.addEventListener("click", this.#onClick);
  }

  disconnectedCallback(): void {
    this.shadowRoot?.removeEventListener("click", this.#onClick);
  }

  readonly #onClick = (event: Event): void => {
    const button = (event.target as HTMLElement | null)?.closest("button");
    if (!button) {
      return;
    }
    if (button.dataset.action === "restart") {
      void this.#restart();
    } else if (button.dataset.action === "reboot") {
      void this.#reboot();
    }
  };

  async #restart(): Promise<void> {
    // Asked, because a restart drops every open terminal and the page it is
    // clicked from shows only the panel. `confirm` is enough: the answer is
    // yes/no and the browser already owns the modal.
    if (!globalThis.confirm("Restart the panel? Open terminals are closed.")) {
      return;
    }
    await this.#run(async (client) => {
      const answer = await client.restart();
      this.#say(`Restarting pid ${answer.pid}. The page reconnects on its own.`, false);
    });
  }

  async #reboot(): Promise<void> {
    const delay = Number(this.shadowRoot?.querySelector<HTMLInputElement>("[data-delay]")?.value ?? 0);
    if (!Number.isFinite(delay) || delay < 0) {
      this.#say("The delay must be a number of seconds.", true);
      return;
    }
    // Deliberately a different question from the restart one: this one says the
    // host goes down, because that is what the operator is agreeing to.
    if (
      !globalThis.confirm(
        `Reboot the host in ${delay}s? Every process on it stops, and nothing in this panel brings it back.`,
      )
    ) {
      return;
    }
    await this.#run(async (client) => {
      const answer = await client.reboot(delay);
      this.#say(
        answer.delay_s > 0
          ? `The host goes down in ${answer.delay_s}s.`
          : "The host is going down now.",
        false,
      );
    });
  }

  /** Runs one action with the busy state and the error report in one place. */
  async #run(action: (client: MaintenanceClient) => Promise<void>): Promise<void> {
    const client = AnthMaintenance.client;
    if (!client) {
      this.#say("No client is configured, so nothing was sent.", true);
      return;
    }
    this.#busy = true;
    this.#said = "";
    this.#paint();
    try {
      await action(client);
    } catch (error) {
      // A `403` from `/api/reboot` is the daemon saying the switch is off. Its
      // own wording names the flag, so it is shown rather than replaced.
      this.#say(error instanceof WireError ? error.message : String(error), true);
    } finally {
      this.#busy = false;
      this.#paint();
    }
  }

  #say(message: string, isError: boolean): void {
    this.#said = message;
    this.#saidIsError = isError;
    this.#paint();
  }

  #paint(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }
    root.innerHTML = `
      <style>${STYLE}</style>
      <section class="card">
        <h2>Restart the panel</h2>
        <p>Replaces the daemon process and re-reads the settings file. The host keeps running.</p>
        <ul class="cost">
          <li>An edit to the settings takes effect.</li>
          <li>Open terminals close: they are children of the old process.</li>
          <li>Your session stays valid: the account and its key are on disk.</li>
        </ul>
        <div class="row">
          <button type="button" data-action="restart"${this.#busy ? " disabled" : ""}>
            Restart panel
          </button>
        </div>
      </section>
      <section class="card">
        <h2>Reboot the host</h2>
        <p>Reboots the whole machine. This does not apply any setting.</p>
        <ul class="cost">
          <li>Every process on the host stops, not just this panel.</li>
          <li>Nothing in this panel brings it back: a supervisor has to.</li>
          <li>Refused unless the daemon was started with <code>--allow-reboot</code>.</li>
        </ul>
        <div class="row">
          <label>Delay
            <input type="number" min="0" max="300" step="1" value="5" data-delay
                   aria-label="Seconds before the host goes down"${this.#busy ? " disabled" : ""}>
            s
          </label>
          <button type="button" class="danger" data-action="reboot"${this.#busy ? " disabled" : ""}>
            Reboot host
          </button>
        </div>
      </section>
      <p class="said${this.#saidIsError ? " err" : " ok"}">${escape(this.#said)}</p>
    `;
  }
}

customElements.define("anth-maintenance", AnthMaintenance);
