/**
 * `anth-app`: the shell, and the one panel v0.2.0 has.
 *
 * v0.1.0 grew a registry, a hash router and a window manager. They are not here
 * yet on purpose: a single panel proves the client, the route and the render,
 * and a registry without a second panel would be an empty abstraction.
 */

import { formatKib, formatUptime, type HostInfo } from "@anth/shared";

import type { HostInfoClient } from "./host-info-client";

export class AnthApp extends HTMLElement {
  /** Set before mounting by `main`, or replaced by a test. */
  static client: HostInfoClient | undefined;

  #info: HostInfo | undefined;
  #error = "";
  #loading = false;

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    this.render();
    // The element is connected by `customElements.define` while `app.ts` is
    // evaluated, which is before the composition root in `main.ts` assigns the
    // client. Deferring one microtask lets that assignment happen first, so a
    // mounted element does not report a client that is about to arrive.
    queueMicrotask(() => void this.refresh());
  }

  async refresh(): Promise<void> {
    const client = AnthApp.client;
    if (!client) {
      this.#error = "No client was configured";
      this.render();
      return;
    }

    this.#loading = true;
    this.#error = "";
    this.render();
    try {
      this.#info = await client.read();
    } catch (error) {
      this.#error = error instanceof Error ? error.message : String(error);
    }
    this.#loading = false;
    this.render();
  }

  private render(): void {
    if (!this.shadowRoot) {
      return;
    }
    this.shadowRoot.innerHTML = `
      <style>
        :host { display: block; font: 13px/1.55 system-ui, -apple-system, sans-serif;
                color: #1c1c1e; background: #f2f2f5; min-height: 100vh; box-sizing: border-box; }
        header { display: flex; align-items: baseline; gap: 12px; padding: 14px 20px;
                 background: #fff; border-bottom: 1px solid #dcdce1; }
        h1 { margin: 0; font-size: 15px; font-weight: 600; letter-spacing: -.01em; }
        .version { font-size: 11px; color: #6b6b70; }
        main { padding: 18px 20px; max-width: 940px; }
        .card { background: #fff; border: 1px solid #dcdce1; border-radius: 10px; padding: 16px 18px; }
        .card-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
        .label { font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: #6b6b70; }
        button { font: inherit; padding: 3px 11px; border-radius: 6px; border: 1px solid #c8c8ce;
                 background: #f7f7f9; cursor: pointer; }
        button:disabled { cursor: progress; opacity: .55; }
        dl { display: grid; grid-template-columns: 150px 1fr; gap: 7px 16px; margin: 0; }
        dt { color: #6b6b70; }
        dd { margin: 0; font-variant-numeric: tabular-nums; }
        .error { margin: 0; padding: 10px 12px; border-radius: 8px; background: #fdeceb;
                 border: 1px solid #f3c9c6; color: #b3261e; }
      </style>
      <header>
        <h1>Anthracite Panel</h1>
        <span class="version">v0.2.0</span>
      </header>
      <main>
        <section class="card">
          <div class="card-head">
            <span class="label">Host</span>
            <button type="button" ${this.#loading ? "disabled" : ""}>Refresh</button>
          </div>
          ${this.body()}
        </section>
      </main>
    `;

    this.shadowRoot.querySelector("button")?.addEventListener("click", () => {
      void this.refresh();
    });
  }

  private body(): string {
    if (this.#error) {
      return `<p class="error" role="alert">${escape(this.#error)}</p>`;
    }
    const info = this.#info;
    if (!info) {
      return `<p class="label">Reading the host…</p>`;
    }

    const used = info.memory.total_kib - info.memory.available_kib;
    return `
      <dl>
        ${row("Hostname", info.hostname)}
        ${row("Operating system", `${info.os.name} ${info.os.version}`.trim())}
        ${row("Kernel", info.os.kernel)}
        ${row("Architecture", info.os.arch)}
        ${row("CPU", info.cpu_model)}
        ${row("Logical cores", String(info.logical_cores))}
        ${row("Uptime", formatUptime(info.uptime_seconds))}
        ${row("Load", `${info.load.one.toFixed(2)} · ${info.load.five.toFixed(2)} · ${info.load.fifteen.toFixed(2)}`)}
        ${row("Memory", `${formatKib(used)} of ${formatKib(info.memory.total_kib)} used`)}
        ${row("Swap", info.memory.swap_total_kib === 0 ? "none" : `${formatKib(info.memory.swap_total_kib - info.memory.swap_free_kib)} of ${formatKib(info.memory.swap_total_kib)} used`)}
      </dl>
    `;
  }
}

function row(label: string, value: string): string {
  return `<dt>${escape(label)}</dt><dd>${escape(value)}</dd>`;
}

function escape(text: string): string {
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

if (!customElements.get("anth-app")) {
  customElements.define("anth-app", AnthApp);
}
