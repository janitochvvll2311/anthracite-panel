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

/**
 * What a widget renders: markup, or an element it owns.
 *
 * A string is replaced wholesale on every repaint, which is fine for text. An
 * element is kept: the same instance is left in place and handed to [`update`]
 * instead of being re-created, so a component with its own state — a Lit
 * element, a canvas, anything holding a subscription — survives a reload.
 */
export type WidgetContent = string | HTMLElement;

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

  /**
   * The attributes a parent may set from a template.
   *
   * lit-html binds an attribute by default (`plugin-id="org.anth.hello"`) and
   * only binds a property with a `.` prefix, so a widget that ignored
   * attributes could not be placed from a Lit template at all.
   */
  static observedAttributes = ["plugin-id", "anchor-base"];

  #state: WidgetState = "loading";
  #data?: T;
  #detail = "";
  #client: AnchorClient | undefined;
  #pluginId?: string;
  #anchorBase?: string;
  #content: HTMLElement | undefined;

  /** The anchor client, created once, when the first call needs it. */
  protected get client(): AnchorClient {
    // Read lazily rather than in the constructor: an attribute set by the
    // parent lands after this element is built, and a client built from the
    // empty plugin id would keep that id for the element's whole life.
    this.#client ??= new AnchorClient({
      pluginId: this.pluginId,
      ...(this.anchorBase === "" ? {} : { origin: this.anchorBase }),
    });
    return this.#client;
  }

  /** This widget's plugin id: the instance value, else the subclass's static. */
  get pluginId(): string {
    const fromInstance = this.#pluginId;
    const fromStatic = (this.constructor as typeof PluginWidget).pluginId;
    if (!fromInstance && !fromStatic) {
      throw new Error("a PluginWidget needs a pluginId");
    }
    return fromInstance ?? fromStatic;
  }

  set pluginId(value: string) {
    this.#pluginId = value;
    // The client captured the id it was built with, so a change retires it.
    this.#client = undefined;
    if (this.isConnected) {
      void this.refresh();
    }
  }

  /** Where the anchors are served; empty means this document's origin. */
  get anchorBase(): string {
    return this.#anchorBase ?? (this.constructor as typeof PluginWidget).anchorBase;
  }

  set anchorBase(value: string) {
    this.#anchorBase = value;
    this.#client = undefined;
  }

  attributeChangedCallback(
    name: string,
    _previous: string | null,
    value: string | null,
  ): void {
    if (name === "plugin-id") {
      if (value) {
        this.pluginId = value;
      }
      return;
    }
    this.anchorBase = value ?? "";
  }

  /** Fetches the data this widget renders. Called on connect and on retry. */
  protected abstract load(): Promise<T>;

  /**
   * Turns the data into what to show.
   *
   * Return a string for markup, which is replaced on every repaint. Return an
   * element — usually a component built once and kept on the instance — and the
   * base keeps it mounted, calling [`update`] instead of replacing it. That is
   * what lets a stateful child (a Lit element with its own `@state`) survive a
   * reload, since replacing `innerHTML` would re-create it and lose everything.
   */
  protected abstract view(data: T): WidgetContent;

  /**
   * Hands new data to an element [`view`] already returned.
   *
   * Called on every load after the first. Ignored for markup, which is simply
   * re-rendered.
   */
  protected update?(element: HTMLElement, data: T): void;


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
    // Reconnecting must not rebuild: a widget moved in the DOM keeps its child
    // element, and re-attaching the shadow tree would throw that child away.
    if (this.shadowRoot) {
      return;
    }

    const shadow = this.attachShadow({ mode: "open" });
    shadow.innerHTML = `
      <style>
        :host { display: block; font: 13px/1.5 system-ui, sans-serif; color: #1c1c1e; }
        .anth-plugin-note { margin: 0; color: #6b6b70; }
        .anth-plugin-error { margin: 0; color: #b3261e; }
        .anth-plugin-retry { margin-top: 8px; font: inherit; padding: 3px 10px;
          border-radius: 6px; border: 1px solid #c4c4c9; background: #f6f6f8;
          cursor: pointer; }
        .anth-plugin-retry:disabled { cursor: progress; opacity: .6; }
      </style>
      <div class="anth-plugin-body"></div>
    `;

    const body = shadow.querySelector(".anth-plugin-body") as HTMLElement;
    this.#body = body;
    // One listener, installed once. It is bound to the container rather than to
    // the button because the body's contents are replaced on every state change,
    // and a listener on the button would be discarded with it.
    body.addEventListener("click", (event) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest(".anth-plugin-retry")) {
        void this.refresh();
      }
    });

    this.repaint();
    // Deferred by a microtask: the shell creates the element and appends it in
    // one turn, and a subclass that assigns its client in its constructor would
    // otherwise be read before the assignment landed.
    queueMicrotask(() => void this.refresh());
  }

  /** Reloads, resetting to the loading state first. */
  async refresh(): Promise<void> {
    this.#state = "loading";
    this.#detail = "";
    this.repaint();
    try {
      this.#data = await this.load();
      this.#state = "ready";
    } catch (error) {
      this.#detail = error instanceof Error ? error.message : String(error);
      this.#state = "error";
    }
    this.repaint();
  }

  /**
   * Repaints the current state.
   *
   * Public so a subclass can force a repaint. Named `repaint` rather than
   * `render` because Lit reserves `render()` for its own template method, and a
   * plugin mixing the two would silently override the wrong one.
   */
  repaint(): void {
    const host = this.#body;
    if (!host) {
      return;
    }

    // Every string state goes through `body()` so a subclass that wraps its
    // widget in chrome keeps that chrome in the loading and error states too.
    if (this.#state !== "ready") {
      this.#content = undefined;
      this.#markup =
        this.#state === "error"
          ? `${this.failure(this.#detail)}${this.retryButton()}`
          : this.loading();
      host.innerHTML = this.body();
      return;
    }

    // `view` is called exactly once per repaint: the same call decides both
    // what to show and whether there is a node to keep.
    const content = this.view(this.#data as T);

    if (typeof content === "string") {
      this.#content = undefined;
      this.#markup = content;
      host.innerHTML = this.body();
      return;
    }

    // The same instance means "this node is still what I want": leaving it in
    // place is the whole point, because writing `innerHTML` would re-create it
    // and a stateful child would lose everything it held.
    const kept = content === this.#content;
    if (!kept) {
      this.#content = content;
      host.textContent = "";
      host.append(content);
    }
    // Called on the first mount too: the element arrives empty, so appending it
    // without handing it the data would leave it blank until the next reload.
    this.update?.(content, this.#data as T);
  }

  /** The string `view` returned for the ready state, if it returned one. */
  #markup = "";

  /** The container every state writes into; created with the shadow root. */
  #body: HTMLElement | undefined;

  /**
   * The markup for the current state.
   *
   * A subclass overrides this to wrap the widget in its own chrome; `hello.ts`
   * does. It is only consulted for the markup path — an element is appended
   * as-is, because wrapping it would re-create it.
   */
  protected body(): string {
    return this.#markup;
  }

  private retryButton(): string {
    return `<button class="anth-plugin-retry" type="button">Reintentar</button>`;
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
