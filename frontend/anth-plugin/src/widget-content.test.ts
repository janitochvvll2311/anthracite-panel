import { afterEach, describe, expect, test } from "bun:test";

import { PluginWidget, type WidgetContent } from "./widget";
import { registerWidget } from "./register";

interface Greeting {
  message: string;
}

/** A child element with state of its own, standing in for a Lit component. */
class StatefulChild extends HTMLElement {
  clicks = 0;
  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" }).innerHTML = `<button>click</button>`;
    }
  }
}

if (!customElements.get("stateful-child")) {
  customElements.define("stateful-child", StatefulChild);
}

/** A widget whose `view` is a test's choice. */
class ContentWidget extends PluginWidget<Greeting> {
  protected static override pluginId = "org.anth.content";

  static answer: () => Promise<Greeting> = async () => ({ message: "hola" });
  static content: (data: Greeting) => WidgetContent = (data) => `<p>${data.message}</p>`;
  static updates: string[] = [];

  protected load(): Promise<Greeting> {
    return ContentWidget.answer();
  }

  protected view(data: Greeting): WidgetContent {
    return ContentWidget.content(data);
  }

  protected override update(element: HTMLElement, data: Greeting): void {
    ContentWidget.updates.push(data.message);
    (element as StatefulChild & { message: string }).setAttribute(
      "data-message",
      data.message,
    );
  }
}

function mount(tag = "anth-content-widget"): ContentWidget {
  registerWidget(tag, ContentWidget);
  const element = document.createElement(tag) as ContentWidget;
  document.body.append(element);
  return element;
}

afterEach(() => {
  document.body.innerHTML = "";
  ContentWidget.updates = [];
  ContentWidget.answer = async () => ({ message: "hola" });
  ContentWidget.content = (data) => `<p>${data.message}</p>`;
});

describe("view returning an element", () => {
  test("keeps the same node mounted across reloads", async () => {
    const child = new StatefulChild();
    ContentWidget.content = () => child;

    const element = mount();
    await element.refresh();
    child.clicks = 3;

    ContentWidget.answer = async () => ({ message: "again" });
    await element.refresh();

    const mounted = element.shadowRoot?.querySelector("stateful-child");
    expect(mounted).toBe(child);
    expect(child.clicks).toBe(3);
  });

  test("hands the element the data on the first mount as well", async () => {
    const child = new StatefulChild();
    ContentWidget.content = () => child;

    const element = mount();
    await element.refresh();

    // Without a first `update` the element would mount empty and only fill in
    // on a later reload, which reads as a blank widget.
    expect(child.getAttribute("data-message")).toBe("hola");
  });

  test("replaces the node when the widget returns a different instance", async () => {
    const first = new StatefulChild();
    ContentWidget.content = () => first;

    const element = mount();
    await element.refresh();

    const second = new StatefulChild();
    ContentWidget.content = () => second;
    await element.refresh();

    expect(element.shadowRoot?.querySelector("stateful-child")).toBe(second);
  });

  test("goes back to markup when the state is no longer ready", async () => {
    const child = new StatefulChild();
    ContentWidget.content = () => child;

    const element = mount();
    await element.refresh();

    ContentWidget.answer = async () => {
      throw new Error("refused");
    };
    await element.refresh();

    // A kept element must not survive into the error state, or the widget would
    // still show stale data beside its own failure message.
    expect(element.shadowRoot?.querySelector("stateful-child")).toBeNull();
    expect(element.shadowRoot?.textContent).toContain("refused");
  });

  test("does not reload when a click bubbles from inside the kept element", async () => {
    const child = new StatefulChild();
    ContentWidget.content = () => child;
    const element = mount();
    await element.refresh();

    let loads = 0;
    ContentWidget.answer = async () => {
      loads += 1;
      return { message: "x" };
    };
    child.shadowRoot
      ?.querySelector("button")
      ?.dispatchEvent(new Event("click", { bubbles: true, composed: true }));
    await Promise.resolve();
    await Promise.resolve();

    expect(loads).toBe(0);
  });
});

describe("view returning markup", () => {
  test("still replaces the markup on every repaint", async () => {
    const element = mount();
    await element.refresh();
    expect(element.shadowRoot?.textContent).toContain("hola");

    ContentWidget.answer = async () => ({ message: "changed" });
    await element.refresh();
    expect(element.shadowRoot?.textContent).toContain("changed");
  });

  test("never calls update", async () => {
    const element = mount();
    await element.refresh();
    await element.refresh();
    expect(ContentWidget.updates).toEqual([]);
  });
});

describe("pluginId as an attribute", () => {
  test("reads the id from the attribute, which is how lit-html binds it", async () => {
    class AttributeWidget extends PluginWidget<Greeting> {
      protected load(): Promise<Greeting> {
        return Promise.resolve({ message: "x" });
      }
      protected view(): string {
        return "";
      }
      /** Exposed so the test can read the id the client would use. */
      get id(): string {
        return this.pluginId;
      }
    }
    registerWidget("anth-attribute-widget", AttributeWidget);

    const element = document.createElement("anth-attribute-widget") as AttributeWidget;
    element.setAttribute("plugin-id", "org.anth.fromattribute");
    document.body.append(element);

    expect(element.id).toBe("org.anth.fromattribute");
  });

  test("retires the client built from the previous id", async () => {
    class ClientWidget extends PluginWidget<Greeting> {
      protected load(): Promise<Greeting> {
        return Promise.resolve({ message: "x" });
      }
      protected view(): string {
        return "ok";
      }
      get url(): (path: string) => string {
        return (path) => this.client.url(path);
      }
    }
    registerWidget("anth-client-widget", ClientWidget);

    const element = document.createElement("anth-client-widget") as ClientWidget;
    element.setAttribute("plugin-id", "org.anth.first");
    document.body.append(element);

    expect(element.url("/x")).toContain("org.anth.first");

    element.setAttribute("plugin-id", "org.anth.second");
    expect(element.url("/x")).toContain("org.anth.second");
  });
});

describe("repaint", () => {
  test("is named repaint so it cannot collide with Lit's render", () => {
    expect("render" in PluginWidget.prototype).toBe(false);
    expect(typeof PluginWidget.prototype.repaint).toBe("function");
  });
});
