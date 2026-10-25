/**
 * The hello plugin's UI.
 *
 * A plugin module is loaded with a dynamic `import()` by the shell and expected
 * to register its custom element as a side effect. The contract, the endpoint
 * client, the load states and the escaping come from `@anth/plugin`; what is left
 * here is this plugin's behaviour.
 *
 * Written as a plain HTMLElement implementing `PluginWidget`, rather than a
 * subclass: the contract is an interface, so the element's own inheritance is
 * free. Here it is `HTMLElement`; `hello-lit.ts` spends it on `LitElement`.
 */

import {
  PluginEndpoints,
  escapeHtml,
  mountWidget,
  registerWidget,
  type MarkupWidget,
} from "@anth/plugin";

import { ANCHOR, type Greeting } from "./anchor";

export { ANCHOR, type Greeting };

const TAG = "anth-hello";

export class AnthHello extends HTMLElement implements MarkupWidget<Greeting> {
  /** The plugin's own client. `pluginId` may be the alias the manifest declares. */
  readonly endpoints = new PluginEndpoints({ pluginId: "org.anth.hello" });

  /** The plumbing: shadow root, load states, retry. */
  readonly handle = mountWidget(this, this);

  load(): Promise<Greeting> {
    return this.endpoints.get<Greeting>(ANCHOR);
  }

  loading(): string {
    return `<p class="label">Reading the host…</p>`;
  }

  view(data: Greeting): string {
    return `
      <div class="card">
        <div class="label">Hello plugin</div>
        <div class="message">${escapeHtml(data.message)}</div>
      </div>
    `;
  }

  /** The widget's own chrome; the states and the retry are `mountWidget`'s. */
  body(markup: string): string {
    return `<style>
      /* Tokens with a fallback: the shell defines them on :root, so a widget
         mounted in the panel inherits its theme, and one opened outside the
         shell still renders in the light palette. */
      .card { border: 1px solid var(--panel-border, #d6d6db); border-radius: 8px;
              padding: 12px 14px; color: var(--text, inherit);
              background: var(--widget-bg, transparent); }
      .label { font-size: 11px; letter-spacing: .06em; text-transform: uppercase;
               color: var(--muted, #6b6b70); }
      .message { margin-top: 6px; font-size: 15px; }
    </style>
    ${markup}`;
  }
}

/** The failure text the daemon sends, re-exported so the test can pin it. */
export { anchorError as describeFailure } from "@anth/plugin";

/* Kept as a named export: a consumer of this module may want the escaper. */
export { escapeHtml };

registerWidget(TAG, AnthHello);
