/**
 * `anth-host-info`: the host snapshot, as the panel presents it.
 *
 * A summary strip with the machine's identity, then one card per resource, each
 * with a bar for the number that is easier to read as a proportion than as
 * bytes.
 *
 * The widget owns its own refresh. It subscribes to `/api/host-info/watch` and
 * repaints only its body, so a live card costs the rest of the shell nothing: no
 * route change, no re-render of the navigation, and a plugin widget beside it is
 * not remounted. Two controls sit in its toolbar — the interval and a pause —
 * because a panel that refreshes every second should let the person reading it
 * decide whether it does.
 *
 * It is an element rather than a function because the shell keeps it mounted
 * across refreshes: `info` is assigned, not re-rendered from scratch, so a scroll
 * position inside the card survives a refresh.
 */

import { type HostInfo, type StreamStatus, watch, watchPath } from "@anth/wire";

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

/** The intervals the selector offers, as the labels the daemon parses. */
const INTERVALS = ["1s", "2s", "5s", "10s", "30s"] as const;

export class AnthHostInfo extends HTMLElement {
  #info: HostInfo | undefined;
  #error = "";
  #loading = false;
  #mounted = false;

  #status: StreamStatus = "connecting";
  #interval = "2s";
  #paused = false;

  /**
   * The one live subscription, with the URL it is on.
   *
   * One field rather than a closer plus the URL it belongs to: two fields had to
   * be cleared together in three places, and a `Retry` that cleared only one of
   * them silently did nothing.
   */
  #subscription: { path: string; close: () => void } | undefined;

  /** The frame (style, toolbar, body) must be rebuilt before the next paint. */
  #frameStale = true;

  /**
   * Where the stream lives, or undefined to render only what is assigned.
   *
   * A shell that sets this gets a live card. A test that does not gets the
   * element as a pure renderer, which is what its assertions are about.
   */
  get watchUrl(): string | undefined {
    return this.#watchUrl;
  }

  set watchUrl(value: string | undefined) {
    this.#watchUrl = value;
    // The toolbar exists only when there is a stream, so gaining or losing the
    // URL changes the frame itself rather than a value inside it.
    this.#frameStale = true;
    this.#resubscribe();
  }

  #watchUrl: string | undefined;

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
    this.#resubscribe();
  }

  /**
   * Closing here is what keeps a paused or removed card from holding a
   * connection open. `EventSource` reconnects on its own, so a subscription that
   * outlived the element would keep asking the daemon forever with nothing on
   * screen to show for it.
   */
  disconnectedCallback(): void {
    this.#stop();
  }

  /** The interval a live card refreshes on, as the flag would spell it. */
  get interval(): string {
    return this.#interval;
  }

  set interval(value: string) {
    if (value === this.#interval) {
      return;
    }
    this.#interval = value;
    // Changing the period means a new URL: SSE carries the schedule in the
    // query, so the old subscription cannot be re-timed.
    this.#resubscribe();
  }

  /** Stops or resumes the stream without losing the snapshot on screen. */
  toggle(): void {
    // A card whose stream was refused has nothing to pause, so the button means
    // "try again" instead of flipping a flag. The close clears `#subscription`,
    // so the resume path below reopens the same URL.
    if (this.#status === "closed" && !this.#paused) {
      this.#resubscribe();
      return;
    }
    this.#paused = !this.#paused;
    if (this.#paused) {
      this.#stop();
      this.#status = "closed";
      this.#paint();
    } else {
      this.#resubscribe();
    }
  }

  get paused(): boolean {
    return this.#paused;
  }

  /** Drops the current subscription, if any. */
  #stop(): void {
    this.#subscription?.close();
    this.#subscription = undefined;
  }

  /**
   * Opens the stream for the current URL and interval, or closes it.
   *
   * Idempotent: called on connect, on an interval change, on resume and on a
   * pause, and it must never leave two subscriptions feeding one card.
   */
  #resubscribe(): void {
    const base = this.#watchUrl;
    if (!base || this.#paused || !this.isConnected) {
      // Nothing to open. A caller that just removed the URL still needs the
      // connection it was using closed, and the toolbar it existed for gone.
      this.#stop();
      this.#paint();
      return;
    }

    const path = watchPath(base, this.#interval);
    if (this.#subscription?.path === path) {
      return;
    }

    this.#stop();
    this.#status = "connecting";

    const close = watch<HostInfo>({
      path,
      event: "host-info",
      onSnapshot: (snapshot) => {
        this.#info = snapshot;
        // A frame clears the previous refusal, including the "interrupted"
        // notice from a blip: the data on screen is current again.
        this.#error = "";
        this.#loading = false;
        this.#status = "live";
        this.#paint();
      },
      onStatus: (status) => {
        // `connecting` is set above, before the request exists, so only the
        // transitions a person can see are recorded here.
        if (status === "live" || status === "error" || status === "closed") {
          this.#status = status;
          if (status === "closed") {
            // The subscription is gone, so the "already watching this URL" guard
            // must not stand in the way of Retry: keeping it set made the button
            // a no-op against a stream the browser had abandoned.
            this.#subscription = undefined;
          }
          this.#paint();
        }
      },
      onError: (message) => {
        this.#error = message;
        this.#paint();
      },
    });
    this.#subscription = { path, close };
    this.#paint();
  }

  /**
   * Brings the card up to date, rebuilding the frame only when it changed shape.
   *
   * Two paths rather than one with a guard: a frame arriving every second must
   * cost a body assignment and nothing else, while a first paint has no elements
   * to update and must build them first.
   */
  #paint(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }
    if (!this.#mounted || this.#frameStale) {
      this.#buildFrame(root);
    }
    this.#paintToolbar(root);
    // Only the body is rewritten, so the element and its scroll position stay.
    const body = root.querySelector<HTMLElement>(".body");
    if (body) {
      body.innerHTML = this.#render();
    }
  }

  /** Replaces the style, the toolbar and the body, then attaches the listeners. */
  #buildFrame(root: ShadowRoot): void {
    root.innerHTML = `<style>${STYLE}</style>${this.#toolbar()}<div class="body" part="body"></div>`;
    this.#mounted = true;
    this.#frameStale = false;
    this.#wireToolbar();
  }

  /**
   * Pushes the current values into controls that already exist.
   *
   * Assigning rather than rebuilding is what keeps the interval selector open
   * under the pointer while frames arrive: a toolbar rebuilt every two seconds
   * would close its menu mid-click.
   */
  #paintToolbar(root: ShadowRoot): void {
    const select = root.querySelector<HTMLSelectElement>("[data-interval]");
    if (select && select.value !== this.#interval) {
      select.value = this.#interval;
    }

    const toggle = root.querySelector<HTMLButtonElement>("[data-pause]");
    if (toggle) {
      // One source for the word: `#paint` and `#toolbar` used to spell it
      // separately, so a refused stream showed a button that still said "Pause".
      const label = this.#buttonLabel();
      if (toggle.textContent !== label) {
        toggle.textContent = label;
      }
      toggle.setAttribute("aria-pressed", String(this.#paused));
    }

    // The attribute drives the colour and the text says what is happening: a
    // pill given only the attribute would keep its first word forever.
    const pill = root.querySelector<HTMLElement>("[data-status]");
    if (pill) {
      pill.setAttribute("data-state", this.#status);
      const label = this.#statusText();
      if (pill.textContent !== label) {
        pill.textContent = label;
      }
    }
  }

  /**
   * The live controls, present only when there is a stream to control.
   *
   * A card with no `watchUrl` is a plain renderer — the shell always sets one,
   * and a test that does not would get buttons that cannot do anything. Every
   * assignment to `watchUrl` marks the frame stale, so this is read only when
   * the answer can have changed.
   */
  #toolbar(): string {
    if (!this.#watchUrl) {
      return "";
    }
    const options = INTERVALS.map(
      (interval) =>
        `<option value="${interval}"${interval === this.#interval ? " selected" : ""}>every ${interval}</option>`,
    ).join("");
    return `
      <div class="toolbar" part="toolbar">
        <span class="status" data-status data-state="${this.#status}" role="status">${this.#statusText()}</span>
        <label class="field">
          <span class="field-label">Refresh</span>
          <select data-interval aria-label="Refresh interval">${options}</select>
        </label>
        <button type="button" data-pause aria-pressed="${this.#paused}">${this.#buttonLabel()}</button>
      </div>`;
  }

  /**
   * What the one button does next.
   *
   * `Retry` is its own word because a refused stream is not a pause: the
   * operator did not stop it, the daemon did, and `Resume` would claim they had
   * something to resume. It is also the only way back from a stream
   * `EventSource` has abandoned — it will not retry a `404` on its own, so a
   * card with no Retry would be dead until a page reload.
   */
  #buttonLabel(): string {
    if (this.#paused) {
      return "Resume";
    }
    return this.#status === "closed" ? "Retry" : "Pause";
  }

  /** What the status pill says, in the operator's words. */
  #statusText(): string {
    switch (this.#status) {
      case "live":
        return "live";
      case "connecting":
        return "connecting";
      case "error":
        return "reconnecting";
      case "closed":
        return this.#paused ? "paused" : "stopped";
    }
  }

  /** The toolbar's listeners, attached once with the frame. */
  #wireToolbar(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }
    // `change` rather than `input`: a `<select>` fires `input` on every
    // keystroke of a type-ahead, and restarting the stream on each one would
    // ask the daemon four times while the operator types "10s".
    root.querySelector("[data-interval]")?.addEventListener("change", (event) => {
      this.interval = (event.currentTarget as HTMLSelectElement).value;
    });
    root.querySelector("[data-pause]")?.addEventListener("click", () => {
      this.toggle();
    });
  }

  #render(): string {
    if (this.#error) {
      return `<p class="error" role="alert">${escape(this.#error)}</p>`;
    }
    const info = this.#info;
    if (!info) {
      // Three states, because they call for different things: a paused card
      // waits for a click, a live one waits for a frame, and a card with no
      // stream at all is simply empty.
      if (this.#paused) {
        return `<p class="empty">Paused — press Resume to read the host again.</p>`;
      }
      if (this.#loading || this.#watchUrl) {
        return `<p class="empty">Reading the host…</p>`;
      }
      return `<p class="empty">No host information yet.</p>`;
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

  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 0.5rem 0.75rem;
             margin-bottom: 0.75rem; padding: 0.5rem 0.65rem; border-radius: 8px;
             border: 1px solid var(--panel-border, #223041);
             background: var(--panel, #101721); }
  .toolbar .status { display: inline-flex; align-items: center; gap: 0.35rem;
                     font: 600 10px/1.4 var(--mono, monospace);
                     letter-spacing: 0.08em; text-transform: uppercase;
                     color: var(--muted, #8b9bad); }
  .toolbar .status::before { content: ""; width: 6px; height: 6px; border-radius: 999px;
                             background: currentColor; }
  .toolbar .status[data-state="live"] { color: var(--ok, #34d399); }
  .toolbar .status[data-state="error"] { color: var(--err, #f87171); }
  .toolbar .status[data-state="connecting"] { color: var(--accent, #7dd3fc); }

  .field { display: inline-flex; align-items: center; gap: 0.35rem;
           font: 12px/1.4 var(--mono, monospace); color: var(--muted, #8b9bad); }
  .toolbar select, .toolbar button {
    font: inherit; color: var(--text, #dbe6f2); padding: 2px 8px; border-radius: 6px;
    border: 1px solid var(--panel-border, #223041); background: var(--widget-bg, rgba(16, 23, 33, 0.5)); }
  .toolbar button { cursor: pointer; margin-left: auto; }
  .toolbar button:hover { border-color: var(--accent, #7dd3fc); }
  .toolbar select:focus-visible, .toolbar button:focus-visible {
    outline: 2px solid var(--accent, #7dd3fc); outline-offset: 1px; }

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
