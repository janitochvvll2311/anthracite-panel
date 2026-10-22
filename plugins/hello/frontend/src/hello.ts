/**
 * The hello plugin's UI.
 *
 * A plugin module is loaded with a dynamic `import()` by the shell and expected
 * to register its custom element as a side effect. The registration, the anchor
 * URL, the load states and the escaping come from `@anth/plugin`; what is left
 * here is this plugin's behaviour.
 */

import { PluginWidget, escapeHtml, registerWidget } from "@anth/plugin";

/** The anchor this module answers. Must match the `[[api]]` block it reads. */
export const ANCHOR = "/greeting";

export interface Greeting {
  message: string;
}

const TAG = "anth-hello";

export class AnthHello extends PluginWidget<Greeting> {
  protected static override pluginId = "org.anth.hello";

  protected load(): Promise<Greeting> {
    return this.client.get<Greeting>(ANCHOR);
  }

  protected override loading(): string {
    return `<p class="label">Reading the host…</p>`;
  }

  protected view(data: Greeting): string {
    return `
      <div class="card">
        <div class="label">Hello plugin</div>
        <div class="message">${escapeHtml(data.message)}</div>
      </div>
    `;
  }

  /** The widget's own chrome; the states and the retry are the base class's. */
  protected override body(): string {
    return `<style>
      .card { border: 1px solid #d6d6db; border-radius: 8px; padding: 12px 14px; }
      .label { font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: #6b6b70; }
      .message { margin-top: 6px; font-size: 15px; }
    </style>
    ${super.body()}`;
  }
}

/** The failure text the daemon sends, re-exported so the test can pin it. */
export { anchorError as describeFailure } from "@anth/plugin";

/* Kept as a named export: a consumer of this module may want the escaper. */
export { escapeHtml };

registerWidget(TAG, AnthHello);
