/**
 * A widget that is already a Lit component.
 *
 * This is what the interface buys over a base class. `PluginWidget` is a shape,
 * not a superclass, so this element extends `LitElement` and still *is* a widget:
 * it keeps Lit's own `render()` and its reactive properties instead of fighting a
 * base class for them. `mountWidget` is not involved — Lit already owns the
 * shadow root — the element just loads through `PluginEndpoints` and lets its
 * properties drive the template.
 *
 * Compare with `hello.ts`, which hands off the plumbing and returns markup.
 */

import { LitElement, html, css } from "lit";
import { PluginEndpoints, registerWidget, type PluginWidget } from "@anth/plugin";

// Imports the shared anchor, never `./hello`: that module registers a tag as a
// side effect, and importing it here would register it a second time.
import { ANCHOR, type Greeting } from "./anchor";

const TAG = "anth-hello-lit";

export class AnthHelloLit extends LitElement implements PluginWidget<Greeting> {
  static override properties = {
    message: { type: String },
    detail: { type: String },
    state: { type: String },
  };

  /**
   * The greeting the endpoint answered; Lit re-renders when it changes.
   *
   * `accessor` rather than a plain field: at `target: ES2022` a class field is
   * defined with `Object.defineProperty`, which shadows the accessor Lit installs
   * for `static properties` and silently stops updates.
   */
  accessor message = "";

  /** The refusal's text, or empty when there is nothing to report. */
  accessor detail = "";

  /** The load state. Lit drives the template from it, so no extra repaint. */
  accessor state: "loading" | "ready" | "error" = "loading";

  readonly endpoints = new PluginEndpoints({ pluginId: "org.anth.hello" });

  /**
   * The whole contract a Lit widget owes.
   *
   * `PluginWidget` is one method precisely so this class can satisfy it without
   * a stub `view` it never renders: Lit's own template is the presentation.
   */
  load(): Promise<Greeting> {
    return this.endpoints.get<Greeting>(ANCHOR);
  }

  override async connectedCallback(): Promise<void> {
    super.connectedCallback();
    await this.refresh();
  }

  /** Fetches, then lets the properties re-render. */
  async refresh(): Promise<void> {
    this.state = "loading";
    this.detail = "";
    try {
      const data = await this.load();
      this.message = data.message;
      this.state = "ready";
    } catch (error) {
      this.detail = error instanceof Error ? error.message : String(error);
      this.state = "error";
    }
  }

  static override styles = css`
    /* Every colour is a token with a fallback. The shell declares them on the
       document root, and a custom property crosses the shadow boundary, so this
       widget follows the panel's theme instead of painting a light card on a
       dark page. Outside the shell the fallback keeps it readable. */
    :host {
      display: block;
      color: var(--text, inherit);
      border: 1px solid var(--panel-border, #d6d6db);
      border-radius: 8px;
      padding: 12px 14px;
      font: 13px/1.5 var(--sans, system-ui, sans-serif);
      background: var(--widget-bg, transparent);
    }
    .label {
      font-size: 11px;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--muted, #6b6b70);
    }
    .message {
      margin-top: 6px;
      font-size: 15px;
    }
    .error {
      margin-top: 6px;
      color: var(--err, #b3261e);
    }
    button {
      margin-top: 10px;
      font: inherit;
      padding: 3px 10px;
      border-radius: 6px;
      color: var(--text, inherit);
      border: 1px solid var(--panel-border, #c4c4c9);
      background: var(--widget-bg, #f6f6f8);
      cursor: pointer;
    }
  `;

  override render() {
    if (this.state === "loading") {
      return html`<div class="label">Hello plugin · Lit</div>
        <div class="message">Cargando…</div>`;
    }
    if (this.state === "error") {
      return html`<div class="label">Hello plugin · Lit</div>
        <div class="error" role="alert">${this.detail}</div>
        <button type="button" @click=${this.#retry}>Reintentar</button>`;
    }
    return html`
      <div class="label">Hello plugin · Lit</div>
      <div class="message">${this.message}</div>
      <button type="button" @click=${this.#count}>
        Clicked ${this.clicks} time${this.clicks === 1 ? "" : "s"}
      </button>
    `;
  }

  /**
   * State no reload can reset.
   *
   * This is the whole point of the example. The widget re-renders from its
   * properties, so a reload updates `message` and leaves this counter alone: the
   * element is never re-created, because nothing replaces it.
   */
  clicks = 0;

  #count = (): void => {
    this.clicks += 1;
    this.requestUpdate();
  };

  #retry = (): void => void this.refresh();
}

registerWidget(TAG, AnthHelloLit);
