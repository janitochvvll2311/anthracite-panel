import { afterEach, describe, expect, test } from "bun:test";

import { PluginWidget, escapeHtml } from "./widget";
import { registerWidget } from "./register";

interface Greeting {
  message: string;
}

/** A widget whose `load` and `view` a test controls. */
class TestWidget extends PluginWidget<Greeting> {
  protected static override pluginId = "org.anth.test";

  static answer: () => Promise<Greeting> = async () => ({ message: "hola" });

  protected load(): Promise<Greeting> {
    return TestWidget.answer();
  }

  protected view(data: Greeting): string {
    return `<p class="greeting">${escapeHtml(data.message)}</p>`;
  }
}

/**
 * happy-dom refuses `new HTMLElement()`; an element comes from the document,
 * which is also how the shell creates one.
 */
function mount(): TestWidget {
  registerWidget("anth-test-widget", TestWidget);
  const element = document.createElement("anth-test-widget") as TestWidget;
  document.body.append(element);
  return element;
}

afterEach(() => {
  document.body.innerHTML = "";
});

/** Lets the deferred load and its render finish. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe("PluginWidget", () => {
  test("renders the loading state before the answer arrives", async () => {
    let release: (value: Greeting) => void = () => {};
    TestWidget.answer = () => new Promise<Greeting>((resolve) => (release = resolve));

    const element = mount();
    await Promise.resolve();
    expect(element.shadowRoot?.textContent).toContain("Cargando");

    release({ message: "hola" });
    await settle();
    await settle();
    expect(element.state).toBe("ready");

    element.remove();
  });

  test("renders the data once loaded", async () => {
    TestWidget.answer = async () => ({ message: "hola" });
    const element = mount();
    await settle();
    await settle();

    expect(element.state).toBe("ready");
    expect(element.shadowRoot?.querySelector(".greeting")?.textContent).toBe("hola");

    element.remove();
  });

  test("turns a failure into a message and a retry button", async () => {
    TestWidget.answer = async () => {
      throw new Error("the host refused");
    };
    const element = mount();
    await settle();
    await settle();

    expect(element.state).toBe("error");
    expect(element.shadowRoot?.textContent).toContain("the host refused");
    expect(element.shadowRoot?.querySelector(".anth-plugin-retry")).not.toBeNull();

    element.remove();
  });

  test("a retry that succeeds leaves the error state behind", async () => {
    TestWidget.answer = async () => {
      throw new Error("first failure");
    };
    const element = mount();
    await settle();
    await settle();
    expect(element.state).toBe("error");

    TestWidget.answer = async () => ({ message: "recovered" });
    await element.refresh();
    await settle();

    expect(element.state).toBe("ready");
    expect(element.shadowRoot?.textContent).toContain("recovered");
    expect(element.shadowRoot?.querySelector(".anth-plugin-retry")).toBeNull();

    element.remove();
  });

  test("escapes the detail it puts in the failure markup", async () => {
    TestWidget.answer = async () => {
      throw new Error("<img src=x onerror=alert(1)>");
    };
    const element = mount();
    await settle();
    await settle();

    expect(element.shadowRoot?.querySelector("img")).toBeNull();
    expect(element.shadowRoot?.textContent).toContain("<img src=x");

    element.remove();
  });

  test("a widget without a plugin id says so instead of fetching", () => {
    class Anonymous extends PluginWidget<Greeting> {
      protected load(): Promise<Greeting> {
        return Promise.resolve({ message: "x" });
      }
      protected view(): string {
        return "";
      }
    }
    registerWidget("anth-anonymous-widget", Anonymous);
    const element = document.createElement("anth-anonymous-widget") as Anonymous;
    expect(() => (element as unknown as { client: unknown }).client).toThrow(/pluginId/);
  });
});

describe("escapeHtml", () => {
  test("escapes every character that could end an attribute or an element", () => {
    expect(escapeHtml(`<a href="x">&'`)).toBe("&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
  });
});

describe("registerWidget", () => {
  test("registers once and reports the re-registration honestly", () => {
    class Once extends HTMLElement {}
    expect(registerWidget("anth-once-test", Once)).toBe(true);
    expect(registerWidget("anth-once-test", Once)).toBe(false);
  });

  test("refuses a tag another element already owns", () => {
    class A extends HTMLElement {}
    class B extends HTMLElement {}
    registerWidget("anth-conflict-test", A);
    expect(() => registerWidget("anth-conflict-test", B)).toThrow(/already registered/);
  });

  test("refuses a name that is not a custom element", () => {
    class C extends HTMLElement {}
    expect(() => registerWidget("nocustomdash", C)).toThrow(/not a valid custom element/);
  });
});
