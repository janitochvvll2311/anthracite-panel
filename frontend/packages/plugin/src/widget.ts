/**
 * The widget base class.
 *
 * Every plugin widget repeated the same four things: attach a shadow root, render
 * a loading state, fetch, and turn a failure into a message. This class owns that
 * so a plugin writes `load()`, `view()` and nothing else.
 *
 * It is deliberately not Lit. A plugin is bundled on its own and shipped to a
 * third party, and the panel's Lit version is not part of the contract; a
 * plugin that imported it would break on a panel upgrade. Plain DOM is a smaller
 * surface to keep compatible.
 */

import { AnchorClient, type AnchorClientOptions } from "./anchors";

/**
 * The three states a widget is in.
 *
 * `error` and `ready` are terminal for one load; a retry returns to `loading`.
 */
export type WidgetState = "loading" | "ready" | "error";

export interface WidgetOptions extends AnchorClientOptions {}

/**
 * A plugin widget: fetch, then render, with the states handled.
 *
 * ```ts
 * export class AnthExample extends PluginWidget {
 *   protected async load(): Promise<{ message: string }> {
 *     return this.client.get("/greeting");
 *   }
 *
 *   protected view(data: { message: string }): string {
 *     return `<p>${escapeHtml(data.message)}</p>`;
 *   }
 * }
 * ```
 *
 * A subclass overrides [`load`], [`view`], [`loading`] and [`failure`]. Everything
 * else — the shadow root, the retry button, the escaping rule — is the contract.
 */
export abstract class PluginWidget<T> extends HTMLElement {
  /** Injected by the shell before the element is mounted, when it has a base. */
  static anchorBase = "";

  /** The plugin id, used for the anchor prefix. Set by the subclass or the shell. */
  protected static pluginId = "";

  #state: WidgetState = "loading";
  #data?: T;
  #detail = "";
  #client?: AnchorClient;

  /** The anchor client, created once from the element's own plugin id. */
  protected get client(): AnchorClient {
    this.#client ??= new AnchorClient({
      pluginId: this.pluginId(),
      ...(PluginWidget.anchorBase === "" ? {} : { origin: PluginWidget.anchorBase }),
    });
    return this.#client;
  }

  /** This widget's plugin id; overridable when one module serves several. */
  protected pluginId(): string {
    const fromTag = (this.constructor as typeof PluginWidget).pluginId;
    if (!fromTag) {
      throw new Error("a PluginWidget needs a pluginId");
    }
    return fromTag;
  }

  /** Fetches the data this widget renders. Called on connect and on retry. */
  protected abstract load(): Promise<T>;

  /** Turns the data into HTML. */
  protected abstract view(data: T): string;

  /** The HTML shown while loading. */
  protected loading(): string {
    return `<p class="anth-plugin-note">Cargando…</p>`;
  }

  /**
   * The HTML shown when the load failed.
   *
   * The detail is the daemon's or the guest's own message, never a bare status:
   * an operator reading the panel needs to know *why*.
   */
  protected failure(detail: string): string {
    return `<p class="anth-plugin-error" role="alert">${escapeHtml(detail)}</p>`;
  }

  /** The current state. Read by tests and by a subclass that wants to react. */
  get state(): WidgetState {
    return this.#state;
  }

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    this.render();
    // Deferred by a microtask: the shell creates the element and appends it in
    // one turn, and a subclass that assigns its client in its constructor would
    // otherwise be read before the assignment landed.
    queueMicrotask(() => void this.refresh());
  }

  /** Reloads, resetting to the loading state first. */
  async refresh(): Promise<void> {
    this.#state = "loading";
    this.#detail = "";
    this.render();
    try {
      this.#data = await this.load();
      this.#state = "ready";
    } catch (error) {
      this.#detail = error instanceof Error ? error.message : String(error);
      this.#state = "error";
    }
    this.render();
  }

  /** Re-renders the current state. Public so a subclass can force a repaint. */
  render(): void {
    if (!this.shadowRoot) {
      return;
    }
    this.shadowRoot.innerHTML = `
      <style>
        :host { display: block; font: 13px/1.5 system-ui, sans-serif; color: #1c1c1e; }
        .anth-plugin-note { margin: 0; color: #6b6b70; }
        .anth-plugin-error { margin: 0; color: #b3261e; }
        .anth-plugin-retry { margin-top: 8px; font: inherit; padding: 3px 10px;
          border-radius: 6px; border: 1px solid #c4c4c9; background: #f6f6f8;
          cursor: pointer; }
        .anth-plugin-retry:disabled { cursor: progress; opacity: .6; }
      </style>
      <div class="anth-plugin-body">${this.body()}</div>
    `;
    this.shadowRoot
      .querySelector(".anth-plugin-retry")
      ?.addEventListener("click", () => void this.refresh());
  }

  /** The inner HTML for the current state. */
  protected body(): string {
    switch (this.#state) {
      case "loading":
        return this.loading();
      case "ready":
        // `view` returns markup by contract, so `undefined` would render
        // "undefined" into the panel; an empty string is the honest answer.
        return this.view(this.#data as T) ?? "";
      case "error":
        return `${this.failure(this.#detail)}
          <button class="anth-plugin-retry" type="button">Reintentar</button>`;
    }
  }
}

/**
 * Escapes text for interpolation into `view` markup.
 *
 * Exported because a widget often interpolates one caption and would otherwise
 * write its own copy, which is how an XSS slips into a panel.
 */
export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (character) => {
    switch (character) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&#39;";
    }
  });
}
