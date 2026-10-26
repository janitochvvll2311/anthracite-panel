import { afterEach, describe, expect, test } from "bun:test";

import type { HostInfo } from "@anth/wire";

import { AnthApp } from "./app";
import type { HostInfoClient } from "./host-info-client";
import type { PluginSummary } from "./repl-client";
import type { PluginCatalog, WidgetHost, WidgetLoader } from "./widget-slots";
import { DASHBOARD_SLOT } from "./widget-slots";

const SAMPLE: HostInfo = {
  hostname: "turing",
  os: { name: "Debian GNU/Linux", version: "13", kernel: "6.12.0", arch: "x86_64" },
  uptime_seconds: 90_000,
  cpu_model: "Intel Xeon",
  logical_cores: 8,
  load: { one: 1.5, five: 2.25, fifteen: 3 },
  memory: { total_kib: 8_388_608, available_kib: 6_291_456, swap_total_kib: 0, swap_free_kib: 0 },
};

function mounting(client: HostInfoClient): { element: AnthApp; text: () => string } {
  AnthApp.client = client;
  const element = document.createElement("anth-app") as AnthApp;
  document.body.append(element);
  return { element, text: () => hostInfoText(element) };
}

/**
 * The card's text, read through the component that renders it.
 *
 * The snapshot moved out of the shell's own shadow root into `anth-host-info`,
 * so a plain `textContent` on `anth-app` stopped covering it: the element's
 * content is not in the shell's text, and a test that only read the shell would
 * pass while the card showed nothing.
 */
function hostInfoText(element: AnthApp): string {
  const panel = element.shadowRoot?.querySelector("anth-host-info");
  return `${element.shadowRoot?.textContent ?? ""} ${panel?.shadowRoot?.textContent ?? ""}`;
}

/** The card's rendered body, which is where the values and errors appear. */
function card(element: AnthApp): Element | null | undefined {
  return element.shadowRoot?.querySelector("anth-host-info")?.shadowRoot?.querySelector(".body");
}

/** Waits for the refresh the element starts in `connectedCallback`. */
async function settled(element: AnthApp): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(card(element)?.querySelector("dl, .error")).not.toBeNull();
}

afterEach(() => {
  document.body.innerHTML = "";
  AnthApp.client = undefined;
  AnthApp.widgets = undefined;
});

describe("anth-app", () => {
  test("renders the snapshot the client returns", async () => {
    const { element, text } = mounting({ read: async () => SAMPLE });
    await settled(element);

    expect(text()).toContain("turing");
    expect(text()).toContain("Debian GNU/Linux 13");
    expect(text()).toContain("8");
    // 90_000 s is 1d 1h.
    expect(text()).toContain("1d 1h");
    // Each figure is its own row now: 8 GiB total, 6 GiB available, 2 GiB used.
    expect(text()).toContain("8.00 GiB");
    expect(text()).toContain("6.00 GiB");
    expect(text()).toContain("2.00 GiB");
    // 2 of 8 GiB is 25%, and the bar is labelled with it.
    expect(text()).toContain("25.0%");
    expect(text()).toContain("Not configured");
  });

  test("shows a refused read instead of an empty table", async () => {
    const { element, text } = mounting({
      read: async () => {
        throw new Error("control is off");
      },
    });
    await settled(element);

    expect(text()).toContain("control is off");
    expect(card(element)?.querySelector("dl")).toBeNull();
    expect(card(element)?.querySelector('[role="alert"]')).not.toBeNull();
  });

  test("escapes host-supplied text so a hostname cannot inject markup", async () => {
    const { element, text } = mounting({
      read: async () => ({ ...SAMPLE, hostname: `<img src=x onerror="alert(1)">` }),
    });
    await settled(element);

    expect(card(element)?.querySelector("img")).toBeNull();
    expect(text()).toContain('<img src=x onerror="alert(1)">');
  });

  test("a refresh after a failure recovers", async () => {
    let fail = true;
    const { element, text } = mounting({
      read: async () => {
        if (fail) {
          throw new Error("daemon is down");
        }
        return SAMPLE;
      },
    });
    await settled(element);
    expect(text()).toContain("daemon is down");

    fail = false;
    element.shadowRoot?.querySelector<HTMLButtonElement>("[data-refresh]")?.click();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(card(element)?.querySelector("dl")).not.toBeNull();
    expect(text()).toContain("turing");
  });

  test("says so when no client was configured rather than rendering nothing", async () => {
    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await Promise.resolve();

    expect(hostInfoText(element)).toContain("No client was configured");
  });

  test("a client assigned right after mounting is still used", async () => {
    // The bug this pins: `customElements.define` connects the element while the
    // module is still being evaluated, so the composition root assigns the
    // client a moment later. Reading it synchronously reported "No client".
    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    AnthApp.client = { read: async () => SAMPLE };

    expect(await settled(element)).toBeUndefined();
    expect(hostInfoText(element)).toContain("turing");
  });
});

const PLUGIN: PluginSummary = {
  id: "org.anth.hello",
  name: "Hello",
  version: "0.2.0",
  anchors: ["GET /greeting"],
  ui: [{ slot: DASHBOARD_SLOT, tag: "anth-app-test-widget", module: "web/hello.js" }],
  disabled_reason: null,
};

/** A host whose catalog and loader are plain objects. */
function widgets(
  plugins: PluginSummary[],
  mount: WidgetLoader["mount"] = async () => {},
): WidgetHost & { mounted: Array<{ tag: string; slot: string }> } {
  const mounted: Array<{ tag: string; slot: string }> = [];
  const catalog: PluginCatalog = { list: async () => plugins };
  const loader: WidgetLoader = {
    mount: async (plugin, slot, into) => {
      mounted.push({ tag: slot.tag, slot: slot.slot });
      await mount(plugin, slot, into);
    },
  };
  return { catalog, loader, mounted };
}

/** Lets the two queued microtasks and the awaits inside them settle. */
async function flush(times = 6): Promise<void> {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve();
  }
}

describe("anth-app slots", () => {
  test("mounts a widget that declares the dashboard slot", async () => {
    const host = widgets([PLUGIN]);
    AnthApp.client = { read: async () => SAMPLE };
    AnthApp.widgets = host;

    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await flush();

    expect(host.mounted).toEqual([{ tag: "anth-app-test-widget", slot: DASHBOARD_SLOT }]);
    expect(element.shadowRoot?.querySelector("[data-plugin='org.anth.hello']")).not.toBeNull();
    expect(element.shadowRoot?.textContent).toContain("Hello");
  });

  test("prefers the slot's own label over the plugin name", async () => {
    const labelled: PluginSummary = {
      ...PLUGIN,
      ui: [{ ...PLUGIN.ui![0]!, label: "Greeting" }],
    };
    AnthApp.client = { read: async () => SAMPLE };
    AnthApp.widgets = widgets([labelled]);

    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await flush();

    expect(element.shadowRoot?.textContent).toContain("Greeting");
  });

  test("renders no slot section when no plugin declares one", async () => {
    AnthApp.client = { read: async () => SAMPLE };
    AnthApp.widgets = widgets([]);

    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await flush();

    const container = element.shadowRoot?.querySelector("[data-slots]");
    expect(container?.children).toHaveLength(0);
  });

  test("reports a plugin that failed to mount and keeps the others", async () => {
    // The failing plugin is second, so a loop that gave up would lose it while
    // looking correct for the first. Both must be attempted.
    const broken: PluginSummary = {
      ...PLUGIN,
      id: "org.anth.broken",
      name: "Broken",
      ui: [{ slot: DASHBOARD_SLOT, tag: "anth-app-broken", module: "web/broken.js" }],
    };
    const host = widgets([PLUGIN, broken], async (plugin) => {
      if (plugin.id === "org.anth.broken") {
        throw new Error("did not register");
      }
    });
    AnthApp.client = { read: async () => SAMPLE };
    AnthApp.widgets = host;

    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await flush();

    expect(host.mounted.map((entry) => entry.tag)).toEqual([
      "anth-app-test-widget",
      "anth-app-broken",
    ]);
    const failed = element.shadowRoot?.querySelector("[data-plugin='org.anth.broken']");
    expect(failed?.textContent).toContain("did not register");
    const healthy = element.shadowRoot?.querySelector("[data-plugin='org.anth.hello']");
    expect(healthy?.textContent).not.toContain("did not register");
  });

  test("reports a failed plugin list instead of an empty page", async () => {
    AnthApp.client = { read: async () => SAMPLE };
    AnthApp.widgets = {
      catalog: {
        list: async () => {
          throw new Error("control is off");
        },
      },
      loader: { mount: async () => {} },
    };

    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await flush();

    const container = element.shadowRoot?.querySelector("[data-slots]");
    expect(container?.textContent).toContain("control is off");
  });

  test("a host refresh does not remount the widget", async () => {
    // The bug this pins: a `render()` that rewrote the shadow root would
    // re-create the slot container, and a widget holding a click count would
    // reset on every Refresh. The assertions are in this order on purpose: the
    // first version of this test compared "the node after" with "the node
    // before" and passed against the bug, because both were null once the
    // rewrite had wiped the slots. Asserting the node exists first is what makes
    // the comparison mean something.
    const host = widgets([PLUGIN]);
    AnthApp.client = { read: async () => SAMPLE };
    AnthApp.widgets = host;

    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await flush();
    const before = element.shadowRoot?.querySelector("[data-plugin='org.anth.hello']");
    expect(before).not.toBeNull();

    element.shadowRoot?.querySelector<HTMLButtonElement>("[data-refresh]")?.click();
    await flush();

    expect(host.mounted).toHaveLength(1);
    expect(element.shadowRoot?.querySelector("[data-plugin='org.anth.hello']")).not.toBeNull();
    expect(element.shadowRoot?.querySelector("[data-plugin='org.anth.hello']")).toBe(before);
  });
});

describe("the shell on a narrow screen", () => {
  function styleOf(): string {
    const element = document.createElement("anth-app");
    document.body.append(element);
    return element.shadowRoot?.querySelector("style")?.textContent ?? "";
  }

  function breakpointOf(style: string, kind: "max-width" | "min-width"): number | undefined {
    const match = style.match(new RegExp(`@media \\(${kind}: ([0-9.]+)rem\\)`));
    return match ? Number(match[1]) : undefined;
  }

  function shadowStyleOf(tag: string): string {
    const element = document.createElement(tag);
    document.body.append(element);
    return element.shadowRoot?.querySelector("style")?.textContent ?? "";
  }

  test("the sidebar collapses by width, never by orientation", () => {
    // Orientation contradicts itself: a phone in landscape is 844px wide and
    // would get the sidebar back, while a 768px tablet upright would lose it
    // and hide its navigation behind a tap for no reason. A width query is
    // what decides whether two columns fit. Only the rules are inspected: the
    // stylesheet's own comment says the word "orientation" on purpose.
    const style = styleOf();

    expect(style).not.toMatch(/@media\s*\(orientation/);
    expect(style).toContain("@media (max-width: 48rem)");
    expect(style).toContain(".sidebar { display: none; }");
    expect(style).toContain(".menu { display: grid; }");
  });

  test("the drawer and the FAB stop exactly where the sidebar starts", () => {
    // The three rules are the two halves of one decision. If they disagree, a
    // width exists where the sidebar is hidden and the drawer refuses to open,
    // so the navigation cannot be reached at all; or where the sidebar is
    // visible and the FAB covers its last link.
    const collapsedAt = breakpointOf(styleOf(), "max-width");
    const drawerAt = breakpointOf(shadowStyleOf("anth-nav-drawer"), "min-width");
    const fabAt = breakpointOf(shadowStyleOf("anth-fab"), "min-width");

    expect(collapsedAt).toBe(48);
    expect(drawerAt).toBeGreaterThan(collapsedAt!);
    expect(drawerAt! - collapsedAt!).toBeLessThanOrEqual(0.125);
    expect(fabAt).toBe(drawerAt);
  });

  test("the menu button is a 44px target", () => {
    // The one control a phone needs to reach every page sat at 40px.
    const style = styleOf();

    expect(style).toContain("width: 44px; height: 44px");
  });

  test("a touch pointer enlarges the refresh button", () => {
    // A tablet is wide and still touched, so the size belongs to the pointer
    // rather than to a width query.
    const style = styleOf();
    const coarse = style.slice(style.indexOf("@media (pointer: coarse)"));

    expect(coarse).not.toBe("");
    expect(coarse).toContain("button[data-refresh] { min-height: 44px; }");
  });
});
