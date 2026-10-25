/**
 * The shell's frame, and the two pieces it delegates to.
 *
 * happy-dom does not compute layout, so "the header stays pinned" cannot be
 * asserted here. What can be asserted is the structure that makes it possible:
 * the header is a sibling of the scroll container, never inside it, and `main`
 * is the only element marked as a scroll container. Those fail if someone moves
 * the header into the scrolling region, which is the mistake that produces the
 * bug.
 */

import { afterEach, describe, expect, test } from "bun:test";

import type { HostInfo } from "@anth/wire";

import { AnthApp } from "./app";
import { DESTINATIONS } from "./destinations";
import { AnthNavDrawer } from "./nav-drawer";
import { AnthTabPanel, TAB_CHANGE_EVENT } from "./tab-panel";

const SAMPLE: HostInfo = {
  hostname: "turing",
  os: { name: "Debian GNU/Linux", version: "13", kernel: "6.12.0", arch: "x86_64" },
  uptime_seconds: 90_000,
  cpu_model: "Intel Xeon",
  logical_cores: 8,
  load: { one: 1.5, five: 2.25, fifteen: 3 },
  memory: { total_kib: 8_388_608, available_kib: 6_291_456, swap_total_kib: 0, swap_free_kib: 0 },
};

async function mountShell(): Promise<AnthApp> {
  AnthApp.client = { read: async () => SAMPLE };
  const element = document.createElement("anth-app") as AnthApp;
  document.body.append(element);
  await flush();
  return element;
}

async function flush(times = 8): Promise<void> {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve();
  }
}

afterEach(() => {
  document.body.innerHTML = "";
  AnthApp.client = undefined;
  AnthApp.widgets = undefined;
  location.hash = "";
});

describe("the shell frame", () => {
  test("the header is outside the scroll container, not inside it", async () => {
    const element = await mountShell();
    const root = element.shadowRoot!;
    const header = root.querySelector("header");
    const main = root.querySelector("main");

    expect(header).not.toBeNull();
    expect(main).not.toBeNull();
    // The bug this pins: putting the header inside `main` looks identical on a
    // screenshot and scrolls the title bar away with the content.
    expect(main!.contains(header!)).toBe(false);
    // Its parent is the host itself: the column is header, then body.
    expect(header!.parentNode).toBe(element.shadowRoot);
  });

  test("there is exactly one scroll container and it is main", async () => {
    const element = await mountShell();
    const root = element.shadowRoot!;

    expect(root.querySelectorAll("main")).toHaveLength(1);
    // The body of the frame is `overflow: hidden`; only `main` scrolls.
    expect(root.querySelector(".shell")).not.toBeNull();
    expect(root.querySelector("main")!.id).toBe("main");
  });

  test("styles pin the host to the viewport height rather than the content", async () => {
    const element = await mountShell();
    const style = element.shadowRoot!.querySelector("style")!.textContent ?? "";

    expect(style).toContain("100dvh");
    expect(style).toContain("overflow: hidden");
    // The thing that lets a flex child shrink below its content. Without it the
    // frame grows and the document scrolls.
    expect(style).toContain("min-height: 0");
  });
});

test("the skip link moves focus into the content instead of rewriting the route", async () => {
  const element = await mountShell();
  const root = element.shadowRoot!;
  location.hash = "#/host";

  root.querySelector<HTMLAnchorElement>("[data-skip]")!.click();

  // `href="#main"` cannot reach an id inside a shadow root, and following it
  // would replace the route with `#main`, which the router would then read as a
  // missing destination.
  expect(location.hash).toBe("#/host");
  expect(root.activeElement?.tagName).toBe("MAIN");
});

describe("navigation", () => {
  test("the sidebar and the drawer list the same destinations", async () => {
    const element = await mountShell();
    const root = element.shadowRoot!;

    const sidebar = [...root.querySelectorAll(".sidebar .nav-item")].map((a) => a.textContent?.trim());
    const drawer = [...root.querySelectorAll("anth-nav-drawer .nav-item")].map((a) => a.textContent?.trim());

    expect(sidebar).toHaveLength(DESTINATIONS.length);
    expect(drawer).toEqual(sidebar);
  });

  test("the active destination is marked for assistive technology", async () => {
    const element = await mountShell();
    const root = element.shadowRoot!;
    const active = root.querySelector('.sidebar .nav-item[data-active]');

    expect(active).not.toBeNull();
    expect(active!.getAttribute("aria-current")).toBe("page");
  });

  test("the menu button opens the drawer and the drawer reports its own close", async () => {
    const element = await mountShell();
    const root = element.shadowRoot!;
    const menu = root.querySelector<HTMLElement>(".menu")!;
    const drawer = root.querySelector<AnthNavDrawer>("anth-nav-drawer")!;

    expect(drawer.open).toBe(false);
    expect(menu.getAttribute("aria-expanded")).toBe("false");

    menu.click();
    expect(drawer.open).toBe(true);
    expect(menu.getAttribute("aria-expanded")).toBe("true");

    // The drawer never closes itself: it reports, and the shell decides.
    drawer.dispatchEvent(
      new CustomEvent("anth-drawer-close", { detail: { reason: "escape" }, bubbles: true }),
    );
    expect(drawer.open).toBe(false);
    expect(menu.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("the tab panel", () => {
  test("parks the hidden tab instead of hiding it", () => {
    const panel = document.createElement("anth-tab-panel") as AnthTabPanel;
    const first = document.createElement("section");
    first.dataset.tab = "one";
    first.setAttribute("label", "One");
    const second = document.createElement("section");
    second.dataset.tab = "two";
    second.setAttribute("label", "Two");
    panel.append(first, second);
    panel.setAttribute("active", "one");
    document.body.append(panel);

    expect(first.isConnected).toBe(true);
    // Removed, not `hidden`: a `display: none` panel keeps rendering.
    expect(second.isConnected).toBe(false);
    expect(second.hasAttribute("hidden")).toBe(false);

    panel.active = "two";
    expect(first.isConnected).toBe(false);
    expect(second.isConnected).toBe(true);
    // The parked element keeps its identity, which is what restores its state.
    expect(panel.tab("one")).toBe(first);
  });

  test("reports the tab the user picked", () => {
    const panel = document.createElement("anth-tab-panel") as AnthTabPanel;
    for (const id of ["one", "two"]) {
      const section = document.createElement("section");
      section.dataset.tab = id;
      section.setAttribute("label", id);
      panel.append(section);
    }
    panel.setAttribute("active", "one");
    document.body.append(panel);

    const seen: string[] = [];
    panel.addEventListener(TAB_CHANGE_EVENT, (event) => {
      seen.push((event as CustomEvent<{ id: string }>).detail.id);
    });

    panel.shadowRoot!.querySelectorAll<HTMLButtonElement>(".tab")[1]!.click();
    expect(seen).toEqual(["two"]);
    expect(panel.active).toBe("two");
  });

  test("projects the active tab, which needs a slot in the shadow root", () => {
    // The bug this pins: without a `<slot>` the active section stays connected
    // in the light DOM but is never rendered, so every `isConnected` assertion
    // still passes while the panel shows an empty box. A real browser found it
    // (the card measured 0px tall); this test keeps it from coming back.
    const panel = document.createElement("anth-tab-panel") as AnthTabPanel;
    const section = document.createElement("section");
    section.dataset.tab = "one";
    panel.append(section);
    document.body.append(panel);

    expect(panel.shadowRoot!.querySelector("slot")).not.toBeNull();
    // The tab being a child is not enough; it has to be projected.
    expect(section.assignedSlot).not.toBeNull();
  });

  test("falls back to the first tab when the route names a panel that is gone", () => {
    const panel = document.createElement("anth-tab-panel") as AnthTabPanel;
    const section = document.createElement("section");
    section.dataset.tab = "one";
    panel.append(section);
    panel.setAttribute("active", "removed");
    document.body.append(panel);

    expect(panel.active).toBe("one");
  });
});

describe("the fab belongs to a panel", () => {
  test("the active panel owns the only fab in the document", async () => {
    const element = await mountShell();
    const root = element.shadowRoot!;
    const fabs = root.querySelectorAll("anth-fab");

    expect(fabs).toHaveLength(1);
    // It sits inside the tab's section, so parking the tab takes the fab with
    // it and a hidden panel can never show an actionable button.
    expect(fabs[0]!.closest("section[data-tab='host']")).not.toBeNull();
  });

  test("the fab refreshes the host information", async () => {
    let reads = 0;
    AnthApp.client = {
      read: async () => {
        reads += 1;
        return SAMPLE;
      },
    };
    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await flush();
    expect(reads).toBe(1);

    element.shadowRoot!.querySelector<HTMLElement>("anth-fab")!
      .shadowRoot!.querySelector<HTMLButtonElement>("button")!
      .click();
    await flush();

    expect(reads).toBe(2);
  });

  test("the fab is disabled while a read is in flight", async () => {
    let release: (() => void) | undefined;
    AnthApp.client = {
      read: () =>
        new Promise((resolve) => {
          release = () => resolve(SAMPLE);
        }),
    };
    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await Promise.resolve();
    await Promise.resolve();

    const fab = element.shadowRoot!.querySelector("anth-fab")!;
    expect(fab.hasAttribute("disabled")).toBe(true);

    release?.();
    await flush();
    expect(fab.hasAttribute("disabled")).toBe(false);
  });
});

describe("the dark theme", () => {
  test("the tokens live on the document root, where a widget can inherit them", async () => {
    // A custom property declared inside the shell's shadow root would not reach
    // a plugin widget; on `:root` it does. The file is read rather than imported
    // because a test environment does not parse CSS, and asserting a computed
    // colour here would only assert that happy-dom returned something.
    const css = await Bun.file(new URL("./theme.css", import.meta.url)).text();
    expect(css).toContain(":root");
    for (const token of ["--panel", "--text", "--muted", "--accent", "--widget-bg"]) {
      expect(css).toContain(token);
    }
    // Dark, not the light palette the shell used before this change.
    expect(css).toContain("color-scheme: dark");
  });
});
