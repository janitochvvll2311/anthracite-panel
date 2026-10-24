/**
 * `anth-repl`: the browser half of the prompt.
 *
 * It owns the input, the history and the scrolling; everything a command *means*
 * is in `repl.ts`, so the element stays a view. The backend is injected, which
 * is how the tests drive every command without a daemon — the same seam
 * `AnthApp.client` provides for host info.
 */

import { completeLine } from "./repl-completion";
import type { ReplBackend, ReplLine } from "./repl";
import { helpText, runLine } from "./repl";

export class AnthRepl extends HTMLElement {
  /** Set before mounting by `main`, or replaced by a test. */
  static backend: ReplBackend | undefined;

  #lines: ReplLine[] = [];
  #history: string[] = [];
  #cursor = 0;

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    this.#push({ kind: "help", text: helpText() });
    this.render();
  }

  /** Runs one line, as the input's Enter handler does. */
  async submit(line: string): Promise<void> {
    const backend = AnthRepl.backend;
    if (!backend) {
      this.#push({ kind: "error", text: "No backend was configured" });
      this.render();
      return;
    }

    // `.trim()` so a stray Enter does not fill the transcript with blank
    // entries; the executor decides what an empty line means.
    const trimmed = line.trim();
    this.#push({ kind: "input", text: trimmed });
    // History lives here rather than in the key handler so a call from a test
    // and a typed line behave the same.
    if (trimmed.length > 0) {
      this.#history.push(trimmed);
      this.#cursor = this.#history.length;
    }

    for (const result of await runLine(backend, trimmed)) {
      // An empty answer is a command that printed nothing, and a blank line in
      // the transcript would look like a failure.
      if (result.text.length > 0 || result.kind === "error") {
        this.#push(result);
      }
    }
    this.render();
  }

  /** The previous history entry, for the input's ArrowUp handler. */
  previous(): string | undefined {
    if (this.#cursor === 0) {
      return undefined;
    }
    this.#cursor -= 1;
    return this.#history[this.#cursor];
  }

  /** The next history entry, for the input's ArrowDown handler. */
  next(): string | undefined {
    if (this.#cursor >= this.#history.length - 1) {
      this.#cursor = this.#history.length;
      return "";
    }
    this.#cursor += 1;
    return this.#history[this.#cursor];
  }

  get lines(): readonly ReplLine[] {
    return this.#lines;
  }

  #push(line: ReplLine): void {
    this.#lines.push(line);
  }

  /**
   * Draws the transcript, and builds the input only once.
   *
   * Rebuilding the whole shadow tree per command destroyed the input mid-line,
   * which lost focus and any text already typed. So the frame is built on the
   * first render and the transcript is the only part that is redrawn.
   */
  private render(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }

    if (!root.querySelector(".transcript")) {
      root.innerHTML = `
        <style>
          :host { display: block; font: 13px/1.5 ui-monospace, monospace; }
          .transcript {
            background: #101418; color: #d7dde3; border-radius: 8px;
            padding: 12px; height: 60vh; overflow-y: auto; white-space: pre-wrap;
          }
          .input-line { display: flex; gap: 8px; margin-top: 8px; }
          .prompt { color: #6b7785; }
          input {
            flex: 1; font: inherit; padding: 6px 8px; border-radius: 6px;
            border: 1px solid #2c343d; background: #161b21; color: inherit;
          }
          .error { color: #ff8f8f; }
          .help { color: #8fb4ff; }
          .input { color: #9aa7b4; }
        </style>
        <div class="transcript" part="transcript"></div>
        <div class="input-line">
          <span class="prompt">anth&gt;</span>
          <input part="input" aria-label="command" autocomplete="off" spellcheck="false">
        </div>
      `;

      this.#input()?.addEventListener("keydown", (event) => {
        const input = this.#input();
        if (!input) {
          return;
        }
        const key = (event as KeyboardEvent).key;
        if (key === "Enter") {
          const value = input.value;
          input.value = "";
          void this.submit(value);
        } else if (key === "Tab") {
          // Tab would otherwise move focus out of the prompt, which reads as
          // the console losing the keystroke.
          event.preventDefault();
          const done = completeLine(input.value);
          input.value = done.line;
          input.setSelectionRange(done.line.length, done.line.length);
          if (done.matches.length > 0) {
            this.#push({ kind: "output", text: done.matches.join("  ") });
            this.render();
          }
        } else if (key === "ArrowUp") {
          event.preventDefault();
          const previous = this.previous();
          if (previous !== undefined) {
            input.value = previous;
          }
        } else if (key === "ArrowDown") {
          event.preventDefault();
          const next = this.next();
          if (next !== undefined) {
            input.value = next;
          }
        }
      });
    }

    const transcript = root.querySelector(".transcript");
    if (transcript) {
      transcript.innerHTML = this.#lines
        .map(
          (line) =>
            `<div class="${line.kind}">${escape(line.kind === "input" ? `anth> ${line.text}` : line.text)}</div>`,
        )
        .join("");
      transcript.scrollTop = transcript.scrollHeight;
    }
    this.#input()?.focus();
  }

  #input(): HTMLInputElement | null {
    return this.shadowRoot?.querySelector("input") ?? null;
  }
}

/** Escapes text before it becomes markup, so a plugin cannot inject HTML. */
function escape(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
