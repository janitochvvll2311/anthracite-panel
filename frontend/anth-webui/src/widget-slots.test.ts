import { afterEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import type { PluginSummary, UiSlot } from "./repl-client";
import { DASHBOARD_SLOT, ModuleWidgetLoader, slotsFor, tagName } from "./widget-slots";

const PLUGIN: PluginSummary = {
  id: "org.anth.hello",
  name: "Hello",
  version: "0.2.0",
  anchors: ["GET /greeting"],
  ui: [{ slot: DASHBOARD_SLOT, tag: "anth-test-hello", module: "web/hello.js" }],
  disabled_reason: null,
};

describe("tagName", () => {
  test("accepts a custom element name", () => {
    expect(tagName("anth-hello")).toBe("anth-hello");
    expect(tagName("anth-hello-lit")).toBe("anth-hello-lit");
  });

  test("refuses a name without a hyphen, which would be a built-in element", () => {
    // `createElement("div")` is valid and would silently replace the widget
    // with a real div, so the check has to happen before the element is made.
    expect(() => tagName("div")).toThrow(/not a valid custom element name/);
  });

  test("refuses an uppercase name, which cannot match a registered tag", () => {
    expect(() => tagName("Anth-Hello")).toThrow(/not a valid custom element name/);
  });

  test("refuses markup smuggled through the tag", () => {
    expect(() => tagName("x-<img src=x onerror=alert(1)>")).toThrow(
      /not a valid custom element name/,
    );
  });
});

describe("slotsFor", () => {
  test("returns the slots that name the requested slot", () => {
    const slots = slotsFor(PLUGIN, DASHBOARD_SLOT);
    expect(slots).toHaveLength(1);
    expect(slots[0]?.tag).toBe("anth-test-hello");
  });

  test("ignores a slot declared for somewhere else", () => {
    expect(slotsFor(PLUGIN, "full.page")).toHaveLength(0);
  });

  test("a disabled plugin contributes nothing, even if it declares the slot", () => {
    // A bundle the host refused must not get a mount attempt: its module is
    // unverified, and the reason it was disabled belongs in the plugin list.
    const disabled = { ...PLUGIN, disabled_reason: "the module digest is all zeroes" };
    expect(slotsFor(disabled, DASHBOARD_SLOT)).toHaveLength(0);
  });

  test("a plugin that declares no ui is not an error", () => {
    const { ui: _ui, ...bare } = PLUGIN;
    expect(slotsFor(bare, DASHBOARD_SLOT)).toHaveLength(0);
  });
});

describe("ModuleWidgetLoader", () => {
  const SLOT: UiSlot = PLUGIN.ui![0]!;

  /**
   * A directory the loader can import from for real.
   *
   * `import()` is the code under test in the success and no-registration cases,
   * so the fixtures are actual ES modules on disk rather than a stubbed loader:
   * a fake `import` would only prove the fake agrees with itself. The base is a
   * `file://` URL because the loader builds `<base>/p/<id>/<module>`, and a
   * filesystem path is the one origin a test can create.
   */
  function scratch(label: string): { base: string; write: (module: string, source: string) => void } {
    const root = mkdtempSync(join(tmpdir(), `anth-widget-${label}-`));
    cleanup.push(root);
    return {
      base: `file://${root}`,
      write(module, source) {
        const file = join(root, "p", PLUGIN.id, module);
        mkdirSync(dirname(file), { recursive: true });
        writeFileSync(file, source);
      },
    };
  }

  const cleanup: string[] = [];

  afterEach(() => {
    for (const root of cleanup.splice(0)) {
      rmSync(root, { recursive: true, force: true });
    }
    document.body.innerHTML = "";
  });

  test("mounts the element the module registered, tagged with its slot", async () => {
    const { base, write } = scratch("ok");
    write(
      "web/hello.js",
      `customElements.define("anth-test-hello", class extends HTMLElement {});`,
    );

    const target = document.createElement("div");
    document.body.append(target);

    await new ModuleWidgetLoader(base).mount(PLUGIN, SLOT, target);

    const mounted = target.querySelector("anth-test-hello");
    expect(mounted).not.toBeNull();
    expect(mounted?.getAttribute("data-slot")).toBe(DASHBOARD_SLOT);
  });

  test("names the plugin and the module when the import fails", async () => {
    const { base } = scratch("missing");
    const target = document.createElement("div");

    await expect(new ModuleWidgetLoader(base).mount(PLUGIN, SLOT, target)).rejects.toThrow(
      /plugin `org\.anth\.hello` could not load `web\/hello\.js`/,
    );
    expect(target.children).toHaveLength(0);
  });

  test("refuses a module that imported but registered no element", async () => {
    // The realistic failure: the manifest's tag and the tag the module defines
    // disagree. The import succeeds, so without this check the slot would render
    // as an empty box with no error anywhere.
    const { base, write } = scratch("unregistered");
    write("web/hello.js", `export const nothing = true;`);

    const target = document.createElement("div");

    await expect(
      new ModuleWidgetLoader(base).mount(
        PLUGIN,
        { ...SLOT, tag: "anth-never-registered" },
        target,
      ),
    ).rejects.toThrow(/did not register `<anth-never-registered>`/);
    expect(target.children).toHaveLength(0);
  });
});
