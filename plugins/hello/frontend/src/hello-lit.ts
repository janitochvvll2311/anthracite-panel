/**
 * A widget that keeps a Lit component instead of re-rendering markup.
 *
 * Compare with `hello.ts`, which returns a string and is replaced wholesale on
 * every repaint. Here `view` returns the *same* element instance every time, so
 * the base leaves it mounted and calls `update` with the new data. The card's
 * own `clicks` state therefore survives a reload.
 */

import { PluginWidget, registerWidget } from "@anth/plugin";

// Imports the shared anchor, never `./hello`: that module registers a tag as a
// side effect, and importing it here would register it a second time.
import { ANCHOR, type Greeting } from "./anchor";
import { HelloCard } from "./lit-card";

const TAG = "anth-hello-lit";

export class AnthHelloLit extends PluginWidget<Greeting> {
  protected static override pluginId = "org.anth.hello";

  /**
   * Built once, owned by this widget.
   *
   * Not in `view`: a fresh card per call would be a *different* instance, and
   * the base would correctly treat it as a swap — losing exactly the state this
   * example is about.
   */
  readonly #card = new HelloCard();

  protected load(): Promise<Greeting> {
    return this.client.get<Greeting>(ANCHOR);
  }

  protected view(): HTMLElement {
    return this.#card;
  }

  protected override update(card: HTMLElement, data: Greeting): void {
    (card as HelloCard).message = data.message;
  }
}

registerWidget(TAG, AnthHelloLit);
