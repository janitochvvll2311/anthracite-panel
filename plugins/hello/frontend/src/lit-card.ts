/**
 * The Lit half of the hello example.
 *
 * This module exists to prove one thing: a component with its own state
 * survives a widget reload. It is a plain custom element that happens to be
 * built with Lit, and nothing in `@anth/plugin` imports Lit — the dependency
 * lives here, in the plugin's own bundle, so a panel upgrade cannot break it.
 *
 * `lit` is imported statically because this is a normal module. Real plugins
 * that want a smaller initial payload load it with `import()`, which is what
 * `docs/plugin-uis.md` recommends; the base class is indifferent.
 */

import { LitElement, html, css } from "lit";

export class HelloCard extends LitElement {
  static override properties = {
    message: { type: String },
  };

  /**
   * The greeting the widget hands down; Lit re-renders when it changes.
   *
   * `accessor` rather than a plain field: at `target: ES2022` a class field is
   * defined with `Object.defineProperty`, which shadows the accessor Lit installs
   * for `static properties` and silently stops updates. Lit logs that very
   * warning, and the counter below would be the only thing still working.
   */
  accessor message = "";

  /**
   * State the widget never sees and cannot reset.
   *
   * This is the whole point of the example: reloading the widget updates
   * `message` and leaves this counter alone, because the element is the same
   * node it was before. If `PluginWidget` still wrote `innerHTML`, this would
   * silently return to zero on every retry.
   */
  clicks = 0;

  static override styles = css`
    :host {
      display: block;
      border: 1px solid #d6d6db;
      border-radius: 8px;
      padding: 12px 14px;
      font: 13px/1.5 system-ui, sans-serif;
    }
    .label {
      font-size: 11px;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: #6b6b70;
    }
    .message {
      margin-top: 6px;
      font-size: 15px;
    }
    button {
      margin-top: 10px;
      font: inherit;
      padding: 3px 10px;
      border-radius: 6px;
      border: 1px solid #c4c4c9;
      background: #f6f6f8;
      cursor: pointer;
    }
  `;

  override render() {
    return html`
      <div class="label">Hello plugin · Lit</div>
      <div class="message">${this.message}</div>
      <button type="button" @click=${this.#count}>
        Clicked ${this.clicks} time${this.clicks === 1 ? "" : "s"}
      </button>
    `;
  }

  #count = (): void => {
    this.clicks += 1;
    this.requestUpdate();
  };
}

if (!customElements.get("hello-card")) {
  customElements.define("hello-card", HelloCard);
}
