/**
 * The hello plugin's UI.
 *
 * A plugin module is loaded with a dynamic `import()` by `anth-plugin-slot` and
 * expected to register its custom element as a side effect. It never imports
 * the shell: the only contract is the anchor below, so a plugin cannot reach
 * into host internals by importing them.
 */

/** The anchor this module answers. Must match the `[[api]]` block it reads. */
export const ANCHOR = "/greeting";

export interface Greeting {
  message: string;
}

const TAG = "anth-hello";

export class AnthHello extends HTMLElement {
  /** Base URL of this plugin's anchors, set by the host before mounting. */
  static anchorBase = "";

  #status: "idle" | "loading" | "ready" | "error" = "idle";
  #message = "";

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    this.render();
    void this.load();
  }

  async load(): Promise<void> {
    this.#status = "loading";
    this.render();

    try {
      const response = await fetch(`${AnthHello.anchorBase}${ANCHOR}`, {
        credentials: "same-origin",
        headers: { accept: "application/json" },
      });
      if (!response.ok) {
        throw new Error(await describeFailure(response));
      }
      const body = (await response.json()) as Greeting;
      this.#message = body.message;
      this.#status = "ready";
    } catch (error) {
      this.#message = error instanceof Error ? error.message : String(error);
      this.#status = "error";
    }
    this.render();
  }

  private render(): void {
    if (!this.shadowRoot) {
      return;
    }
    this.shadowRoot.innerHTML = `
      <style>
        :host { display: block; font: 13px/1.5 system-ui, sans-serif; color: #1c1c1e; }
        .card { border: 1px solid #d6d6db; border-radius: 8px; padding: 12px 14px; }
        .label { font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: #6b6b70; }
        .message { margin-top: 6px; font-size: 15px; }
        .message[data-state="error"] { color: #b3261e; }
        button { margin-top: 10px; font: inherit; padding: 3px 10px; border-radius: 6px;
                 border: 1px solid #c4c4c9; background: #f6f6f8; cursor: pointer; }
        button:disabled { cursor: progress; opacity: .6; }
      </style>
      <div class="card">
        <div class="label">Hello plugin</div>
        <div class="message" data-state="${this.#status}">${this.text()}</div>
        <button type="button" ${this.#status === "loading" ? "disabled" : ""}>Refresh</button>
      </div>
    `;
    this.shadowRoot.querySelector("button")?.addEventListener("click", () => {
      void this.load();
    });
  }

  private text(): string {
    switch (this.#status) {
      case "loading":
        return "Reading the host…";
      case "ready":
        return escapeHtml(this.#message);
      case "error":
        return escapeHtml(this.#message);
      default:
        return "";
    }
  }
}

/** The daemon answers `{"error": "..."}` on every failure; surface it verbatim. */
export async function describeFailure(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { error?: string };
    if (body.error) {
      return body.error;
    }
  } catch {
    // A non-JSON body is not worth a second error.
  }
  return `${response.status} ${response.statusText}`;
}

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

if (!customElements.get(TAG)) {
  customElements.define(TAG, AnthHello);
}
