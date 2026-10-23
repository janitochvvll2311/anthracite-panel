/**
 * The widget contract.
 *
 * A plugin's web module is imported by the shell and registers a custom element.
 * That element is the plugin's own — it extends whatever it likes — and what it
 * owes the host is this interface, not a base class. The v0.1.0 base class forced
 * every widget through one inheritance chain, which is why a Lit component could
 * not be a widget and a widget could not be a Lit component: the two fought over
 * `render()` and over who owned the shadow root.
 *
 * So there is no base class here. There is a contract, an optional mounting
 * helper ([`mountWidget`](./mount)), and an endpoint client
 * ([`PluginEndpoints`](./anchors)) a widget can use without being told how.
 *
 * ```ts
 * export class AnthExample extends HTMLElement implements PluginWidget<Greeting> {
 *   readonly endpoints = new PluginEndpoints({ pluginId: "org.example.example" });
 *
 *   constructor() {
 *     super();
 *     mountWidget(this, this);
 *   }
 *
 *   async load(): Promise<Greeting> {
 *     return this.endpoints.get("/greeting");
 *   }
 *
 *   view(data: Greeting): string {
 *     return `<p>${escapeHtml(data.message)}</p>`;
 *   }
 * }
 * ```
 */

/**
 * The three states a widget is in.
 *
 * `error` and `ready` are terminal for one load; a retry returns to `loading`.
 */
export type WidgetState = "loading" | "ready" | "error";

/**
 * What a widget renders: markup, or an element it owns.
 *
 * A string is dropped into the host on every repaint, which is fine for text. An
 * element is kept: the same instance is left in place and handed to [`update`]
 * instead of being re-created, so a component with its own state — a Lit
 * element, a canvas, anything holding a subscription — survives a reload.
 */
export type WidgetContent = string | HTMLElement;

/**
 * The basic contract: a widget knows how to fetch its own data.
 *
 * Deliberately one method. Presentation is the plugin's business — a plain
 * element returning markup, a Lit component rendering from properties, a canvas
 * painter — and pinning it here is what made the v0.1.0 base class unable to host
 * anything but itself. [`MarkupWidget`] adds the markup shape for plugins that
 * want it; [`mountWidget`](./mount) implements that shape for them.
 */
export interface PluginWidget<T = unknown> {
  /** Fetches the data this widget renders. Called on connect and on retry. */
  load(): Promise<T>;
}

/**
 * A widget that renders markup through [`view`], plus the hooks
 * [`mountWidget`](./mount) calls around it.
 *
 * This is the shape to implement when the plugin wants the helper to own the
 * shadow root, the load states and the retry button. It extends
 * [`PluginWidget`], so it is a widget by construction rather than by convention.
 */
export interface MarkupWidget<T = unknown> extends PluginWidget<T> {
  /**
   * Turns the data into what to show.
   *
   * Return a string for markup, which is replaced on every repaint. Return an
   * element — usually a component built once and kept on the instance — and the
   * host keeps it mounted, calling [`update`] instead of replacing it. That is
   * what lets a stateful child (a Lit element with its own `@state`) survive a
   * reload, since replacing `innerHTML` would re-create it and lose everything.
   */
  view(data: T): WidgetContent;

  /**
   * Hands new data to an element [`view`] already returned.
   *
   * Ignored for markup, which is simply re-rendered.
   */
  update?(element: HTMLElement, data: T): void;

  /** The markup shown while loading. */
  loading?(): string;

  /**
   * The markup shown when the load failed.
   *
   * The detail is the daemon's or the guest's own message, never a bare status:
   * an operator reading the panel needs to know *why*. The default escapes it;
   * an override is responsible for doing the same.
   */
  failure?(detail: string): string;

  /**
   * Wraps the widget's markup in its own chrome.
   *
   * Called for every string state, so a plugin that draws a card keeps the card
   * in the loading and error states too. Only consulted for the markup path: an
   * element is appended as-is, because wrapping it would re-create it.
   */
  body?(markup: string): string;
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
