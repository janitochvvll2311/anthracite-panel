import { afterEach, describe, expect, test } from "bun:test";

import type { HostInfo } from "@anth/shared";

import { AnthApp } from "./app";
import type { HostInfoClient } from "./host-info-client";

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
  return { element, text: () => element.shadowRoot?.textContent ?? "" };
}

/** Waits for the refresh the element starts in `connectedCallback`. */
async function settled(element: AnthApp): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(element.shadowRoot?.querySelector("dl, .error")).not.toBeNull();
}

afterEach(() => {
  document.body.innerHTML = "";
  AnthApp.client = undefined;
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
    // 8 GiB total, 6 GiB available is 2 GiB used.
    expect(text()).toContain("2.00 GiB of 8.00 GiB used");
    expect(text()).toContain("none");
  });

  test("shows a refused read instead of an empty table", async () => {
    const { element, text } = mounting({
      read: async () => {
        throw new Error("control is off");
      },
    });
    await settled(element);

    expect(text()).toContain("control is off");
    expect(element.shadowRoot?.querySelector("dl")).toBeNull();
    expect(element.shadowRoot?.querySelector('[role="alert"]')).not.toBeNull();
  });

  test("escapes host-supplied text so a hostname cannot inject markup", async () => {
    const { element, text } = mounting({
      read: async () => ({ ...SAMPLE, hostname: `<img src=x onerror="alert(1)">` }),
    });
    await settled(element);

    expect(element.shadowRoot?.querySelector("img")).toBeNull();
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
    element.shadowRoot?.querySelector("button")?.click();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(element.shadowRoot?.querySelector("dl")).not.toBeNull();
    expect(text()).toContain("turing");
  });

  test("says so when no client was configured rather than rendering nothing", async () => {
    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    await Promise.resolve();

    expect(element.shadowRoot?.textContent).toContain("No client was configured");
  });

  test("a client assigned right after mounting is still used", async () => {
    // The bug this pins: `customElements.define` connects the element while the
    // module is still being evaluated, so the composition root assigns the
    // client a moment later. Reading it synchronously reported "No client".
    const element = document.createElement("anth-app") as AnthApp;
    document.body.append(element);
    AnthApp.client = { read: async () => SAMPLE };

    expect(await settled(element)).toBeUndefined();
    expect(element.shadowRoot?.textContent).toContain("turing");
  });
});
