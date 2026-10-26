import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import type { HostInfo } from "@anth/wire";

import { AnthHostInfo } from "./host-info";

const SAMPLE: HostInfo = {
  hostname: "turing",
  os: { name: "Debian GNU/Linux", version: "13", kernel: "6.12.0", arch: "x86_64" },
  uptime_seconds: 90_000,
  cpu_model: "Intel Xeon",
  logical_cores: 8,
  load: { one: 1.5, five: 2.25, fifteen: 3 },
  memory: { total_kib: 8_388_608, available_kib: 6_291_456, swap_total_kib: 0, swap_free_kib: 0 },
};

/**
 * A card holding `info`.
 *
 * Constructed directly rather than through `document.createElement`. In a test
 * module that imports this file and nothing else, happy-dom can hand back an
 * un-upgraded `HTMLElement` for the tag, whose `shadowRoot` is null and whose
 * text is empty: every assertion below would then pass against nothing. The
 * `shadowRoot` assertion makes that fail loudly instead. The real path — the tag
 * written into the shell's template and upgraded by the parser — is pinned in
 * `app.test.ts`, which mounts `anth-app` and reads the card through it.
 */
function mount(info?: HostInfo): AnthHostInfo {
  const element = new AnthHostInfo();
  document.body.append(element);
  expect(element.shadowRoot).not.toBeNull();
  if (info) {
    element.info = info;
  }
  return element;
}

/**
 * The card's text, shadow roots included.
 *
 * `textContent` does not cross a shadow boundary, and this element keeps its
 * styles and body inside one, so reading it from the host would answer "".
 */
function text(element: AnthHostInfo): string {
  const body = element.shadowRoot?.querySelector(".body");
  return `${body?.textContent ?? ""} ${[...(element.shadowRoot?.querySelectorAll(".bar") ?? [])]
    .map((bar) => bar.getAttribute("aria-label") ?? "")
    .join(" ")}`;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("the host card", () => {
  test("renders one card per resource plus the summary", () => {
    const element = mount(SAMPLE);

    for (const part of ["summary", "cpu", "load", "memory", "swap"]) {
      expect(element.shadowRoot?.querySelector(`[part="${part}"]`)).not.toBeNull();
    }
  });

  test("prints the identity of the machine", () => {
    expect(text(mount(SAMPLE))).toContain("turing");
    expect(text(mount(SAMPLE))).toContain("Debian GNU/Linux");
    expect(text(mount(SAMPLE))).toContain("13");
    expect(text(mount(SAMPLE))).toContain("6.12.0");
    expect(text(mount(SAMPLE))).toContain("x86_64");
  });

  test("shows memory as total, used and available", () => {
    const shown = text(mount(SAMPLE));

    // 8 GiB total, 6 GiB available, so 2 GiB used.
    expect(shown).toContain("8.00 GiB");
    expect(shown).toContain("6.00 GiB");
    expect(shown).toContain("2.00 GiB");
    expect(shown).toContain("25.0% used");
  });

  test("says swap is not configured rather than drawing an empty bar", () => {
    const element = mount(SAMPLE);
    const swap = element.shadowRoot?.querySelector('[part="swap"]');

    expect(swap?.textContent).toContain("Not configured");
    expect(swap?.querySelector(".bar")).toBeNull();
  });

  test("draws a swap bar when there is swap", () => {
    const withSwap: HostInfo = {
      ...SAMPLE,
      memory: { ...SAMPLE.memory, swap_total_kib: 2_097_152, swap_free_kib: 1_048_576 },
    };
    const swap = mount(withSwap).shadowRoot?.querySelector('[part="swap"]');

    expect(swap?.textContent).toContain("2.00 GiB");
    expect(swap?.textContent).toContain("50.0% used");
    expect(swap?.querySelector(".bar")).not.toBeNull();
  });

  test("a bar fills a percentage of its track and never overflows it", () => {
    // Memory larger than the total is impossible from the daemon and reachable
    // from a hand-written fixture; the bar must clamp instead of painting past
    // its track.
    const over: HostInfo = {
      ...SAMPLE,
      memory: { ...SAMPLE.memory, available_kib: 0 },
    };
    const bar = mount(over).shadowRoot?.querySelector<HTMLElement>('[part="memory"] .bar > span');
    expect(bar?.getAttribute("style")).toContain("100%");

    const under: HostInfo = { ...SAMPLE, memory: { ...SAMPLE.memory, available_kib: 9_000_000 } };
    const empty = mount(under).shadowRoot?.querySelector<HTMLElement>('[part="memory"] .bar > span');
    expect(empty?.getAttribute("style")).toContain("0%");
  });

  test("every bar is announced, not just drawn", () => {
    const element = mount(SAMPLE);
    const bars = [...element.shadowRoot!.querySelectorAll('[role="img"]')];

    // One memory bar and three load bars. A bar a screen reader skips is a
    // number only a sighted user can read.
    expect(bars).toHaveLength(4);
    for (const bar of bars) {
      expect(bar.getAttribute("aria-label") ?? "").not.toBe("");
    }
  });

  test("the load bars are relative to the cores, not to the load", () => {
    // 4.0 on 8 cores is half, so the 15-minute bar is 50% wide.
    const loaded: HostInfo = { ...SAMPLE, load: { one: 8, five: 4, fifteen: 2 } };
    const bars = [...mount(loaded).shadowRoot!.querySelectorAll<HTMLElement>('[part="load"] .bar-row .bar > span')];

    expect(bars.map((bar) => bar.getAttribute("style")?.trim())).toEqual([
      "width: 100%",
      "width: 50%",
      "width: 25%",
    ]);
  });

  test("escapes a hostname so it cannot inject markup", () => {
    const element = mount({ ...SAMPLE, hostname: `<img src=x onerror="alert(1)">` });

    expect(element.shadowRoot?.querySelector("img")).toBeNull();
    expect(text(element)).toContain('<img src=x onerror="alert(1)">');
  });

  test("escapes the OS strings too, not only the hostname", () => {
    const element = mount({
      ...SAMPLE,
      os: { ...SAMPLE.os, name: `<script>bad()</script>`, version: `"1"` },
    });

    expect(element.shadowRoot?.querySelector("script")).toBeNull();
    expect(text(element)).toContain("<script>bad()</script>");
  });
});

describe("the card's states", () => {
  test("an error replaces the card instead of sitting above stale values", () => {
    const element = mount(SAMPLE);
    element.error = "daemon is down";

    expect(text(element)).toContain("daemon is down");
    expect(element.shadowRoot?.querySelector("dl")).toBeNull();
    expect(element.shadowRoot?.querySelector('[role="alert"]')).not.toBeNull();
  });

  test("clearing the error brings the card back", () => {
    const element = mount(SAMPLE);
    element.error = "daemon is down";
    element.error = "";

    expect(element.shadowRoot?.querySelector("dl")).not.toBeNull();
    expect(text(element)).toContain("turing");
  });

  test("the empty state says which of reading and idle it is", () => {
    const element = mount();
    expect(text(element)).toContain("No host information yet");

    element.loading = true;
    expect(text(element)).toContain("Reading the host");
  });

  test("an empty CPU model is named rather than left blank", () => {
    expect(text(mount({ ...SAMPLE, cpu_model: "" }))).toContain("Unknown CPU");
  });

  test("an empty kernel or arch is named rather than left blank", () => {
    const shown = text(mount({ ...SAMPLE, os: { ...SAMPLE.os, kernel: "", arch: "" } }));
    expect(shown.match(/unknown/g)).toHaveLength(2);
  });
});

/** The card's own stylesheet. */
function styleOf(element: AnthHostInfo): string {
  return element.shadowRoot?.querySelector("style")?.textContent ?? "";
}

describe("the card on a narrow screen", () => {
  test("the grid can shrink to a single column", () => {
    // `minmax(190px, 1fr)` alone makes the grid wider than a 360px phone once
    // padding is counted, because the track's minimum is a fixed 190px and the
    // container's is not consulted. `min(190px, 100%)` is what lets it shrink.
    const style = styleOf(mount());

    expect(style).toContain("minmax(min(190px, 100%), 1fr)");
  });

  test("a phone gets one column and larger text", () => {
    const style = styleOf(mount());
    const phone = style.slice(style.indexOf("@media (max-width: 30rem)"));

    expect(phone).not.toBe("");
    expect(phone).toContain("grid-template-columns: minmax(0, 1fr)");
    expect(phone).toContain("font-size: 13px");
  });

  test("the phone rule is keyed on width, not orientation", () => {
    // A 768px tablet in portrait fits three columns; an `orientation` query
    // would give it one and waste the row.
    const style = styleOf(mount());

    expect(style).not.toContain("orientation: portrait");
  });

  test("long values wrap instead of widening the card", () => {
    const style = styleOf(mount());

    // A CPU model or a hostname can be one long token with no spaces; without
    // this the card would push the whole shell sideways.
    expect(style).toContain("overflow-wrap: anywhere");
    expect(style).toContain("min-width: 0");
  });
});

/**
 * The card's own refresh.
 *
 * `watch` is driven through the real `EventSource` shape rather than a spy on
 * the element's internals, and the assertions are about what a person would see:
 * a frame replaces the numbers, the selector re-subscribes, pause closes the
 * subscription and resume opens a fresh one.
 *
 * The fake carries `readyState` because `EventSource` reports two different
 * failures through one event: a dropped socket it will retry (`CONNECTING`) and
 * a reply it has given up on (`CLOSED`), such as the `404` a daemon without the
 * watch route answers.
 */
class FakeEventSource {
  static opened: FakeEventSource[] = [];
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;

  readonly url: string;
  closed = false;
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  #listeners = new Map<string, ((event: MessageEvent<string>) => void)[]>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.opened.push(this);
  }

  addEventListener(name: string, listener: (event: MessageEvent<string>) => void): void {
    this.#listeners.set(name, [...(this.#listeners.get(name) ?? []), listener]);
  }

  close(): void {
    this.closed = true;
    this.readyState = FakeEventSource.CLOSED;
  }

  emit(name: string, data: string): void {
    for (const listener of this.#listeners.get(name) ?? []) {
      listener({ data } as MessageEvent<string>);
    }
  }

  static last(): FakeEventSource {
    const source = FakeEventSource.opened.at(-1);
    if (!source) {
      throw new Error("the card opened no stream");
    }
    return source;
  }
}

const realEventSource = globalThis.EventSource;

/** A card the shell has connected to a stream. */
function liveCard(): AnthHostInfo {
  const element = mount();
  element.watchUrl = "/api/host-info";
  return element;
}

beforeEach(() => {
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
});

afterEach(() => {
  FakeEventSource.opened = [];
  globalThis.EventSource = realEventSource;
  document.body.innerHTML = "";
});

describe("the card's live stream", () => {
  test("subscribes to the watch route at the documented default", () => {
    liveCard();

    expect(FakeEventSource.last().url).toBe("/api/host-info/watch?interval=2s");
  });

  test("the toolbar appears and goes with the URL that needs it", () => {
    const element = mount(SAMPLE);
    expect(element.shadowRoot?.querySelector("[data-pause]")).toBeNull();

    element.watchUrl = "/api/host-info";
    expect(element.shadowRoot?.querySelector("[data-pause]")).not.toBeNull();

    // Losing the URL means the buttons have nothing to control, so they must not
    // be left on screen promising an action.
    element.watchUrl = undefined;
    expect(element.shadowRoot?.querySelector("[data-pause]")).toBeNull();
    expect(FakeEventSource.last().readyState).toBe(FakeEventSource.CLOSED);
  });

  test("a frame replaces the card's numbers without a full re-render", () => {
    const element = liveCard();
    const body = element.shadowRoot?.querySelector(".body");
    FakeEventSource.last().emit(
      "host-info",
      JSON.stringify({ ...SAMPLE, hostname: "after-the-frame" }),
    );

    // The same node, repainted: a re-render would replace `.body` and lose a
    // scroll position inside a long card.
    expect(element.shadowRoot?.querySelector(".body")).toBe(body);
    expect(text(element)).toContain("after-the-frame");
    expect(text(element)).not.toContain("turing");
  });

  test("the status pill follows the stream", () => {
    const element = liveCard();
    const status = () => element.shadowRoot?.querySelector("[data-status]");

    expect(status()?.getAttribute("data-state")).toBe("connecting");

    const source = FakeEventSource.last();
    source.onopen?.();
    expect(status()?.getAttribute("data-state")).toBe("live");
    // The word, not only the attribute: a pill that changed colour but kept
    // saying "connecting" is the bug this asserts against.
    expect(status()?.textContent).toBe("live");

    source.onerror?.();
    expect(status()?.getAttribute("data-state")).toBe("error");
    expect(status()?.textContent).toBe("reconnecting");
    // An interrupted stream is reported, not hidden: the next frame clears it.
    expect(text(element)).toContain("interrupted");

    source.emit("host-info", JSON.stringify(SAMPLE));
    expect(status()?.getAttribute("data-state")).toBe("live");
    expect(status()?.textContent).toBe("live");
    expect(text(element)).not.toContain("interrupted");
  });

  test("choosing another interval re-subscribes at the new period", () => {
    const element = liveCard();
    const first = FakeEventSource.last();

    element.interval = "10s";

    expect(first.closed).toBe(true);
    expect(FakeEventSource.last().url).toBe("/api/host-info/watch?interval=10s");
  });

  test("the selector is what changes the interval", () => {
    const element = liveCard();
    const select = element.shadowRoot?.querySelector<HTMLSelectElement>("[data-interval]");
    expect(select).not.toBeNull();
    // The offered values are the labels the daemon parses, so what the browser
    // sends is what the CLI flag takes.
    expect([...(select?.options ?? [])].map((option) => option.value)).toContain("10s");

    select!.value = "5s";
    select!.dispatchEvent(new Event("change"));

    expect(FakeEventSource.last().url).toBe("/api/host-info/watch?interval=5s");
  });

  test("a frame arriving mid-selection does not close the selector", () => {
    const element = liveCard();
    const select = element.shadowRoot?.querySelector<HTMLSelectElement>("[data-interval]");

    FakeEventSource.last().emit("host-info", JSON.stringify(SAMPLE));

    // The toolbar is built once; only its state is reassigned. Rebuilding it on
    // every frame would close the menu under the operator's pointer.
    expect(element.shadowRoot?.querySelector("[data-interval]")).toBe(select);
  });

  test("pause closes the stream and resume opens a fresh one", () => {
    const element = liveCard();
    const first = FakeEventSource.last();

    element.toggle();
    expect(first.closed).toBe(true);
    expect(element.paused).toBe(true);
    expect(
      element.shadowRoot?.querySelector("[data-status]")?.getAttribute("data-state"),
    ).toBe("closed");
    // The numbers stay on screen: pausing stops the refresh, it does not blank
    // the card.
    element.info = SAMPLE;
    expect(text(element)).toContain("turing");
    expect(FakeEventSource.opened).toHaveLength(1);

    element.toggle();
    expect(element.paused).toBe(false);
    expect(FakeEventSource.opened).toHaveLength(2);
    expect(FakeEventSource.last().url).toBe("/api/host-info/watch?interval=2s");
  });

  test("the pause button is what pauses", () => {
    const element = liveCard();
    element.shadowRoot?.querySelector<HTMLButtonElement>("[data-pause]")?.click();

    expect(element.paused).toBe(true);
    expect(element.shadowRoot?.querySelector<HTMLButtonElement>("[data-pause]")?.textContent).toBe(
      "Resume",
    );
  });

  test("removing the card closes its subscription", () => {
    const element = liveCard();
    const first = FakeEventSource.last();

    element.remove();

    // An `EventSource` outlives the element otherwise, reconnecting forever for
    // a card nobody can see.
    expect(first.closed).toBe(true);
  });

  test("a card with no watch URL renders but never subscribes", () => {
    mount(SAMPLE);

    expect(FakeEventSource.opened).toHaveLength(0);
  });

  test("the first frame fills a card the shell left empty", () => {
    const element = liveCard();
    expect(text(element)).toContain("Reading the host");

    FakeEventSource.last().emit("host-info", JSON.stringify(SAMPLE));

    expect(text(element)).toContain("turing");
    expect(text(element)).not.toContain("Reading the host");
  });
});

/**
 * When the stream cannot be opened at all.
 *
 * The fake keeps the browser's lifecycle value honest: `CLOSED` is what
 * `EventSource` reports after a reply it will not retry, such as the `404` a
 * daemon without the route answers.
 */
describe("a card whose stream was refused", () => {
  test("says so instead of promising a reconnection that never comes", () => {
    const element = liveCard();
    const source = FakeEventSource.last();
    source.readyState = 2;
    source.onerror?.();

    const status = element.shadowRoot?.querySelector("[data-status]");
    expect(status?.getAttribute("data-state")).toBe("closed");
    expect(status?.textContent).toBe("stopped");
    expect(text(element)).toContain("refused");
    // The old behaviour, and the bug: a pill that said "reconnecting" forever
    // against a route that answers `404`.
    expect(text(element)).not.toContain("reconnecting");
  });

  test("offers a retry, because the daemon may have been restarted since", () => {
    const element = liveCard();
    const source = FakeEventSource.last();
    source.readyState = 2;
    source.onerror?.();

    const button = element.shadowRoot?.querySelector<HTMLButtonElement>("[data-pause]");
    expect(button?.textContent).toBe("Retry");

    // What the operator restarts a stream with is the button in front of them.
    button?.click();

    expect(FakeEventSource.opened).toHaveLength(2);
    expect(element.paused).toBe(false);
  });

  test("a retryable drop still reads as reconnecting", () => {
    const element = liveCard();
    const source = FakeEventSource.last();
    source.readyState = 0;
    source.onerror?.();

    const status = element.shadowRoot?.querySelector("[data-status]");
    expect(status?.getAttribute("data-state")).toBe("error");
    expect(status?.textContent).toBe("reconnecting");
  });
});
