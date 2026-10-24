/**
 * `anth-app`: the shell.
 *
 * It owns two things: the host panel it renders itself, and the slots a plugin
 * declares. v0.1.0 grew a registry, a hash router and a window manager; they are
 * not here yet on purpose, so the shell is one page with one named slot rather
 * than an abstraction with no second user.
 *
 * The frame is built once and the host card is repainted into it. That matters
 * for the slots: a widget is a live element, and a `render()` that rewrote the
 * whole shadow root would re-create it on every refresh, losing whatever state
 * it held.
 */

import type { HostInfo } from "@anth/wire";

import { formatKib, formatUptime } from "./wire";

import type { HostInfoClient } from "./host-info-client";
import { DASHBOARD_SLOT, type WidgetHost, slotsFor } from "./widget-slots";

export class AnthApp extends HTMLElement {
  /** Set before mounting by `main`, or replaced by a test. */
  static client: HostInfoClient | undefined;

  /**
   * Where plugin widgets come from, or undefined for a shell with no plugins.
   *
   * One seam rather than two static fields: a catalog without a loader cannot
   * mount anything, and an absent catalog means "no plugins", which is the
   * state a test that only cares about the host card wants.
   */
  static widgets: WidgetHost | undefined;

  #info: HostInfo | undefined;
  #error = "";
  #loading = false;
  #mounted = false;

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
    queueMicrotask(() => void this.mountWidgets());
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
    // The frame is written once. A widget mounted into it is a live element, so
    // rewriting the shadow root on a refresh would discard it and its state.
    if (!this.#mounted) {
      this.shadowRoot.innerHTML = `
        <style>
          ${STYLE}
        </style>
        <header>
          <h1>Anthracite Panel</h1>
          <span class="version">v0.2.0</span>
        </header>
        <main>
          <section class="card" data-host>
          </section>
          <div data-slots></div>
        </main>
      `;
      this.#mounted = true;
    }

    const host = this.shadowRoot.querySelector<HTMLElement>("[data-host]");
    if (!host) {
      return;
    }
    host.innerHTML = `
      <div class="card-head">
        <span class="label">Host</span>
        <button type="button" ${this.#loading ? "disabled" : ""}>Refresh</button>
      </div>
      ${this.body()}
    `;
    host.querySelector("button")?.addEventListener("click", () => {
      void this.refresh();
    });
  }

  /**
   * Mounts every plugin that declares the dashboard slot.
   *
   * One plugin failing must not cost the others their place on the page, so a
   * refusal is reported per plugin and the loop continues. A section with no
   * widgets stays out of the frame entirely rather than rendering an empty box.
   */
  private async mountWidgets(): Promise<void> {
    const host = AnthApp.widgets;
    const container = this.shadowRoot?.querySelector<HTMLElement>("[data-slots]");
    if (!host || !container) {
      return;
    }

    let plugins;
    try {
      plugins = await host.catalog.list();
    } catch (error) {
      container.innerHTML = `<p class="error" role="alert">${escape(
        `reading the plugin list failed: ${describe(error)}`,
      )}</p>`;
      return;
    }

    container.innerHTML = "";
    for (const plugin of plugins) {
      for (const slot of slotsFor(plugin, DASHBOARD_SLOT)) {
        const section = document.createElement("section");
        section.className = "card slot";
        section.dataset.plugin = plugin.id;
        section.dataset.slot = slot.slot;
        const heading = document.createElement("div");
        heading.className = "card-head";
        const label = document.createElement("span");
        label.className = "label";
        label.textContent = slot.label ?? plugin.name;
        heading.append(label);
        container.append(section);
        section.append(heading);

        const target = document.createElement("div");
        section.append(target);
        try {
          await host.loader.mount(plugin, slot, target);
        } catch (error) {
          target.innerHTML = `<p class="error" role="alert">${escape(
            describe(error),
          )}</p>`;
        }
      }
    }
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

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const STYLE = `
  :host { display: block; font: 13px/1.55 system-ui, -apple-system, sans-serif;
          color: #1c1c1e; background: #f2f2f5; min-height: 100vh; box-sizing: border-box; }
  header { display: flex; align-items: baseline; gap: 12px; padding: 14px 20px;
           background: #fff; border-bottom: 1px solid #dcdce1; }
  h1 { margin: 0; font-size: 15px; font-weight: 600; letter-spacing: -.01em; }
  .version { font-size: 11px; color: #6b6b70; }
  main { padding: 18px 20px; max-width: 940px; }
  .card { background: #fff; border: 1px solid #dcdce1; border-radius: 10px; padding: 16px 18px; }
  .slot { margin-top: 14px; }
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
`;

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
