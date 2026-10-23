/**
 * Mounting a widget into an element.
 *
 * The counterpart of [`PluginWidget`](./contract): the widget supplies `load` and
 * `view`, this module owns the shadow root, the load states, the retry button and
 * the decision to keep an element or replace markup. A plugin that wants all of
 * that calls it once and writes no plumbing; a plugin that already has a
 * framework — a Lit component with its own `render()` — can skip it entirely and
 * still be a widget.
 *
 * ```ts
 * export class AnthExample extends HTMLElement implements PluginWidget<Greeting> {
 *   readonly widget = mountWidget(this, this);
 *   …
 * }
 * ```
 */

import { escapeHtml, type MarkupWidget, type WidgetContent, type WidgetState } from "./contract";

/** What a mounted widget exposes to its own element. */
export interface WidgetHandle {
  /** Reloads, resetting to the loading state first. */
  refresh(): Promise<void>;
  /** Repaints the current state. Call after changing something `view` reads. */
  repaint(): void;
  /** The current state. */
  readonly state: WidgetState;
}

const mounted = new WeakMap<HTMLElement, WidgetHandle>();

const STYLE = `
  :host { display: block; font: 13px/1.5 system-ui, sans-serif; color: #1c1c1e; }
  .anth-plugin-note { margin: 0; color: #6b6b70; }
  .anth-plugin-error { margin: 0; color: #b3261e; }
  .anth-plugin-retry { margin-top: 8px; font: inherit; padding: 3px 10px;
    border-radius: 6px; border: 1px solid #c4c4c9; background: #f6f6f8;
    cursor: pointer; }
  .anth-plugin-retry:disabled { cursor: progress; opacity: .6; }
`;

/**
 * Mounts `widget` into `host`, attaching a shadow root.
 *
 * Idempotent: a second call for the same host returns the handle it already
 * built. That matters because a custom element's constructor and its
 * `connectedCallback` are both plausible places to call this, and a widget moved
 * in the DOM calls neither cleanly twice.
 */
export function mountWidget<T>(host: HTMLElement, widget: MarkupWidget<T>): WidgetHandle {
  const existing = mounted.get(host);
  if (existing) {
    return existing;
  }

  const shadow = host.shadowRoot ?? host.attachShadow({ mode: "open" });
  shadow.innerHTML = `<style>${STYLE}</style><div class="anth-plugin-body"></div>`;
  const body = shadow.querySelector(".anth-plugin-body") as HTMLElement;

  let state: WidgetState = "loading";
  let data: T | undefined;
  let detail = "";
  let content: HTMLElement | undefined;
  let markup = "";

  const render = (): void => {
    if (state !== "ready") {
      content = undefined;
      markup =
        state === "error"
          ? `${failure(widget, detail)}${RETRY_BUTTON}`
          : (widget.loading?.() ?? DEFAULT_LOADING);
      body.innerHTML = wrap(widget, markup);
      return;
    }

    // Called exactly once per repaint: the same call decides both what to show
    // and whether there is a node to keep.
    const next: WidgetContent = widget.view(data as T);

    if (typeof next === "string") {
      content = undefined;
      markup = next;
      body.innerHTML = wrap(widget, markup);
      return;
    }

    // The same instance means "this node is still what I want": leaving it in
    // place is the whole point, because writing `innerHTML` would re-create it
    // and a stateful child would lose everything it held.
    if (next !== content) {
      content = next;
      body.textContent = "";
      body.append(next);
    }
    // Called on the first mount too: the element arrives empty, so appending it
    // without handing it the data would leave it blank until the next reload.
    widget.update?.(next, data as T);
  };

  // One listener, installed once. It is bound to the container rather than to the
  // button because the body's contents are replaced on every state change, and a
  // listener on the button would be discarded with it. `closest` is scoped to the
  // body, so a click inside a kept component never counts as a retry.
  body.addEventListener("click", (event) => {
    const target = event.target as HTMLElement | null;
    if (target?.closest(".anth-plugin-retry")) {
      void handle.refresh();
    }
  });

  const handle: WidgetHandle = {
    async refresh(): Promise<void> {
      state = "loading";
      detail = "";
      render();
      try {
        data = await widget.load();
        state = "ready";
      } catch (error) {
        detail = error instanceof Error ? error.message : String(error);
        state = "error";
      }
      render();
    },
    repaint: render,
    get state(): WidgetState {
      return state;
    },
  };

  mounted.set(host, handle);
  render();
  // Deferred by a microtask: the widget is still in its constructor, and its own
  // fields are assigned after the `super()` call that mounts it.
  queueMicrotask(() => void handle.refresh());

  return handle;
}

const DEFAULT_LOADING = `<p class="anth-plugin-note">Cargando…</p>`;

const RETRY_BUTTON = `<button class="anth-plugin-retry" type="button">Reintentar</button>`;

function failure<T>(widget: MarkupWidget<T>, detail: string): string {
  if (widget.failure) {
    return widget.failure(detail);
  }
  return `<p class="anth-plugin-error" role="alert">${escapeHtml(detail)}</p>`;
}

function wrap<T>(widget: MarkupWidget<T>, markup: string): string {
  return widget.body ? widget.body(markup) : markup;
}
