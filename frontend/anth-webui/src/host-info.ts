/**
 * `anth-host-info`: the host snapshot, as the panel presents it.
 *
 * This is the v0.1.0 card brought forward: a summary strip with the machine's
 * identity, then one card per resource, each with a bar for the number that is
 * easier to read as a proportion than as bytes. The toolbar and the SSE
 * controls are gone with the stream — v0.2.0 has no watch route, so refresh is
 * the shell's button and the FAB, and this element only renders what it is
 * given.
 *
 * It is an element rather than a function because the shell keeps it mounted
 * across refreshes: `info` is assigned, not re-rendered from scratch, so a
 * scroll position inside the card survives a refresh.
 */

import type { HostInfo } from "@anth/wire";

import { escape } from "./html";
import {
  formatKib,
  formatLoad,
  formatPercent,
  formatUptime,
  loadPercent,
  memoryUsedKib,
  swapUsedKib,
  usagePercent,
} from "./wire";

export class AnthHostInfo extends HTMLElement {
  #info: HostInfo | undefined;
  #error = "";
  #loading = false;
  #mounted = false;

  /** The snapshot to show, or undefined for "nothing read yet". */
  get info(): HostInfo | undefined {
    return this.#info;
  }

  set info(value: HostInfo | undefined) {
    this.#info = value;
    this.#paint();
  }

  /** A refused read, shown instead of an empty card. */
  get error(): string {
    return this.#error;
  }

  set error(value: string) {
    this.#error = value;
    this.#paint();
  }

  /** True while a read is in flight, which is what the empty state reports. */
  get loading(): boolean {
    return this.#loading;
  }

  set loading(value: boolean) {
    this.#loading = value;
    this.#paint();
  }

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    this.#paint();
  }

  #paint(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }
    if (!this.#mounted) {
      root.innerHTML = `<style>${STYLE}</style><div class="body" part="body"></div>`;
      this.#mounted = true;
    }
    // Only the body is rewritten, so the element and its scroll position stay.
    const body = root.querySelector<HTMLElement>(".body");
    if (body) {
      body.innerHTML = this.#render();
    }
  }

  #render(): string {
    if (this.#error) {
      return `<p class="error" role="alert">${escape(this.#error)}</p>`;
    }
    const info = this.#info;
    if (!info) {
      return `<p class="empty">${this.#loading ? "Reading the host…" : "No host information yet."}</p>`;
    }

    return `${this.#summary(info)}
      <div class="grid" part="details">
        <article class="card" part="cpu">
          <h3>CPU</h3>
          <p class="model">${escape(info.cpu_model || "Unknown CPU")}</p>
          <p class="detail">${info.logical_cores} logical cores</p>
        </article>
        ${this.#load(info)}
        ${this.#memory(info)}
        ${this.#swap(info)}
      </div>`;
  }

  #summary(info: HostInfo): string {
    const version = info.os.version ? ` <span>${escape(info.os.version)}</span>` : "";
    return `
      <section class="summary" part="summary">
        <div class="who">
          <p class="eyebrow">Host</p>
          <h2>${escape(info.hostname)}</h2>
          <p class="os">${escape(info.os.name)}${version}</p>
        </div>
        <dl class="quick">
          ${pair("Kernel", info.os.kernel || "unknown")}
          ${pair("Architecture", info.os.arch || "unknown")}
          ${pair("Uptime", formatUptime(info.uptime_seconds))}
        </dl>
      </section>`;
  }

  #load(info: HostInfo): string {
    const loads: readonly [string, number][] = [
      ["1 min", info.load.one],
      ["5 min", info.load.five],
      ["15 min", info.load.fifteen],
    ];
    const rows = loads
      .map(([label, value]) => {
        const percent = loadPercent(value, info.logical_cores);
        return `<div class="bar-row">
            <span class="bar-label">${label}</span>
            ${bar(
              `${label} load: ${formatLoad(value)}, ${formatPercent(percent)} of ${info.logical_cores} cores`,
              percent,
            )}
          </div>`;
      })
      .join("");

    return `
      <article class="card" part="load">
        <h3>Load average</h3>
        <dl>${loads.map(([label, value]) => pair(label, formatLoad(value))).join("")}</dl>
        <div class="bars">${rows}</div>
      </article>`;
  }

  #memory(info: HostInfo): string {
    const used = memoryUsedKib(info.memory);
    const percent = usagePercent(used, info.memory.total_kib);
    return `
      <article class="card" part="memory">
        <h3>Memory</h3>
        <dl>
          ${pair("Total", formatKib(info.memory.total_kib))}
          ${pair("Used", formatKib(used))}
          ${pair("Available", formatKib(info.memory.available_kib))}
        </dl>
        ${bar(`Memory used: ${formatPercent(percent)}`, percent)}
        <p class="detail">${formatPercent(percent)} used</p>
      </article>`;
  }

  #swap(info: HostInfo): string {
    if (info.memory.swap_total_kib === 0) {
      return `
        <article class="card" part="swap">
          <h3>Swap</h3>
          <p class="detail">Not configured</p>
        </article>`;
    }
    const used = swapUsedKib(info.memory);
    const percent = usagePercent(used, info.memory.swap_total_kib);
    return `
      <article class="card" part="swap">
        <h3>Swap</h3>
        <dl>
          ${pair("Total", formatKib(info.memory.swap_total_kib))}
          ${pair("Used", formatKib(used))}
          ${pair("Free", formatKib(info.memory.swap_free_kib))}
        </dl>
        ${bar(`Swap used: ${formatPercent(percent)}`, percent)}
        <p class="detail">${formatPercent(percent)} used</p>
      </article>`;
  }
}

function pair(label: string, value: string): string {
  return `<div><dt>${escape(label)}</dt><dd>${escape(value)}</dd></div>`;
}

function bar(label: string, percent: number): string {
  return `<div class="bar" role="img" aria-label="${escape(label)}" title="${escape(label)}">
    <span style="width: ${percent}%"></span>
  </div>`;
}

/*
 * The grid is intrinsic: it asks for 190px cards and gives up when the column is
 * narrower, so it reflows on its own between the 940px desktop column, the
 * ~340px left after the sidebar, and a 360px phone. `min(190px, 100%)` is what
 * stops a narrow column from overflowing, which `minmax(190px, 1fr)` alone does
 * not. The one breakpoint below is for the phone, where a single column also
 * wants larger type and taller bars.
 */
const STYLE = `
  :host { display: block; color: var(--text, #dbe6f2);
          font: 14px/1.45 var(--sans, system-ui, sans-serif); }

  .body { display: flex; flex-direction: column; gap: 0.75rem; }

  .summary { display: flex; flex-wrap: wrap; align-items: end; justify-content: space-between;
             gap: 0.9rem 1rem; padding: 0.85rem 1rem; border-radius: 8px;
             border: 1px solid var(--panel-border, #223041);
             background: var(--panel, #101721); }

  .who { min-width: 0; }
  h2 { margin: 0.15rem 0 0.2rem; overflow-wrap: anywhere;
       font: 600 clamp(1.35rem, 3.4vw, 2rem)/1.1 var(--mono, monospace);
       letter-spacing: -0.03em; }
  .eyebrow { margin: 0; color: var(--muted, #8b9bad);
             font: 600 10px/1.4 var(--mono, monospace);
             letter-spacing: 0.1em; text-transform: uppercase; }
  .os { margin: 0; color: var(--muted, #8b9bad); font-size: 13px; }
  .os span { margin-left: 0.25rem; }

  .grid { display: grid; gap: 0.65rem;
          grid-template-columns: repeat(auto-fit, minmax(min(190px, 100%), 1fr)); }

  .card { min-width: 0; padding: 0.75rem 0.9rem; border-radius: 8px;
          border: 1px solid var(--panel-border, #223041);
          background: var(--widget-bg, rgba(16, 23, 33, 0.5)); }

  h3 { margin: 0 0 0.5rem; color: var(--accent, #7dd3fc);
       font: 600 11px/1.4 var(--mono, monospace);
       letter-spacing: 0.08em; text-transform: uppercase; }

  /* One column of label/value pairs, wrapped so a long value (a CPU model, an
     architecture name) takes its own line instead of squeezing the label. */
  dl { display: grid; gap: 0.2rem; margin: 0; }
  dl div { display: flex; flex-wrap: wrap; gap: 0.2rem 0.5rem; justify-content: space-between;
           align-items: baseline; }

  .quick { gap: 0.35rem 1rem; grid-template-columns: repeat(auto-fit, minmax(8.5rem, auto)); }
  .quick div { justify-content: start; gap: 0.45rem; }

  dt { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
  dd { margin: 0; overflow-wrap: anywhere;
       font: 12px/1.4 var(--mono, monospace); font-variant-numeric: tabular-nums; }

  .model { margin: 0 0 0.25rem; overflow-wrap: anywhere; }
  .detail, .empty { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
  .detail { margin: 0.35rem 0 0; }

  .bars { display: grid; gap: 0.3rem; margin-top: 0.5rem; }
  .bar-row { display: grid; grid-template-columns: 2.75rem minmax(0, 1fr);
             align-items: center; gap: 0.5rem; }

  .bar { display: block; height: 6px; overflow: hidden; border-radius: 999px;
         background: rgba(139, 155, 173, 0.22); }
  .bar > span { display: block; height: 100%; border-radius: inherit;
                background: linear-gradient(90deg, var(--accent, #7dd3fc), var(--ok, #34d399)); }

  .bar-label { color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); }

  .empty { margin: 0; padding: 1.5rem 0.75rem; text-align: center;
           border: 1px dashed var(--panel-border, #223041); border-radius: 8px; }

  .error { margin: 0; padding: 0.6rem 0.75rem; border-radius: 8px;
           border: 1px solid rgba(248, 113, 113, 0.4);
           background: rgba(248, 113, 113, 0.12); color: var(--err, #f87171); }

  /* A phone: the summary stacks, and the text gets one step larger because the
     card is the whole viewport rather than a column in a grid. Keyed on width,
     not orientation: a 768px tablet in portrait fits three columns and would
     waste two-thirds of the row. 30rem is 480px at the default root size, which
     is a phone in landscape and the largest phone in portrait. */
  @media (max-width: 30rem) {
    .summary { align-items: start; padding: 0.75rem; }
    .quick { grid-template-columns: minmax(0, 1fr); }
    .quick div { justify-content: space-between; }
    .grid { grid-template-columns: minmax(0, 1fr); }
    dd, .detail { font-size: 13px; }
    .bar { height: 8px; }
  }
`;

if (!customElements.get("anth-host-info")) {
  customElements.define("anth-host-info", AnthHostInfo);
}
